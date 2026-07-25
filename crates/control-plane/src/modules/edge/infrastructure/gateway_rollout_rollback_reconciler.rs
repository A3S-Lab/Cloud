use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::infrastructure::{
    CompileGatewayRolloutRollback, GatewayRollbackMemberSnapshotContext,
    GatewayRolloutRollbackCompiler,
};
use crate::modules::shared_kernel::domain::{GatewayRolloutId, NodeId, RepositoryError};
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRolloutRollbackReconciliationFailure {
    pub failed_rollout_id: GatewayRolloutId,
    pub node_id: Option<NodeId>,
    pub operation: &'static str,
    pub error: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GatewayRolloutRollbackReconciliationReport {
    pub required_rollbacks: usize,
    pub staged_rollbacks: usize,
    pub replayed_rollbacks: usize,
    pub superseded_rollbacks: usize,
    pub failures: Vec<GatewayRolloutRollbackReconciliationFailure>,
}

pub struct GatewayRolloutRollbackReconciler {
    repository: Arc<dyn IEdgeRepository>,
    compiler: GatewayRolloutRollbackCompiler,
    interval: Duration,
    batch_size: usize,
}

impl GatewayRolloutRollbackReconciler {
    pub fn new(
        repository: Arc<dyn IEdgeRepository>,
        compiler: GatewayRolloutRollbackCompiler,
        interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero() || batch_size == 0 || batch_size > 10_000 {
            return Err(
                "Gateway rollback reconciliation requires a positive interval and bounded batch"
                    .into(),
            );
        }
        Ok(Self {
            repository,
            compiler,
            interval,
            batch_size,
        })
    }

    pub async fn run_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<GatewayRolloutRollbackReconciliationReport, RepositoryError> {
        let targets = self
            .repository
            .pending_gateway_rollout_rollbacks(self.batch_size)
            .await?;
        let mut report = GatewayRolloutRollbackReconciliationReport {
            required_rollbacks: targets.len(),
            ..GatewayRolloutRollbackReconciliationReport::default()
        };
        for target in targets {
            let failed_rollout_id = target.failed_rollout.id;
            let member_contexts = match self.member_contexts(&target.scope).await {
                Ok(contexts) => contexts,
                Err((node_id, operation, error)) => {
                    report
                        .failures
                        .push(failure(failed_rollout_id, node_id, operation, error));
                    continue;
                }
            };
            let compiled = match self.compiler.compile(CompileGatewayRolloutRollback {
                scope: target.scope,
                failed_rollout: target.failed_rollout,
                rollback: target.rollback,
                member_contexts,
                issued_at: now,
            }) {
                Ok(compiled) => compiled,
                Err(_) => {
                    report.failures.push(failure(
                        failed_rollout_id,
                        None,
                        "compile",
                        "Gateway exact rollback compilation failed",
                    ));
                    continue;
                }
            };
            let bundle = match compiled.stage_bundle() {
                Ok(bundle) => bundle,
                Err(_) => {
                    report.failures.push(failure(
                        failed_rollout_id,
                        None,
                        "validate",
                        "Gateway exact rollback stage bundle is invalid",
                    ));
                    continue;
                }
            };
            match self.repository.stage_gateway_rollout_rollback(bundle).await {
                Ok(result) => {
                    report.staged_rollbacks += 1;
                    report.replayed_rollbacks += usize::from(result.replayed);
                }
                Err(_) => match self
                    .repository
                    .find_gateway_rollout_rollback(
                        compiled.scope.organization_id,
                        failed_rollout_id,
                    )
                    .await
                {
                    Ok(rollback)
                        if rollback.state
                            != crate::modules::edge::domain::GatewayRolloutRollbackState::Required =>
                    {
                        report.superseded_rollbacks += 1;
                    }
                    _ => report.failures.push(failure(
                        failed_rollout_id,
                        None,
                        "stage",
                        "Gateway exact rollback persistence failed",
                    )),
                },
            }
        }
        Ok(report)
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once(Utc::now()).await {
                        Ok(report) => {
                            for failure in report.failures {
                                tracing::warn!(
                                    failed_gateway_rollout_id = %failure.failed_rollout_id,
                                    gateway_node_id = failure.node_id.map(|node_id| node_id.to_string()),
                                    operation = failure.operation,
                                    error = failure.error,
                                    "Gateway exact rollback reconciliation failed"
                                );
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "Gateway exact rollback scan failed"
                        ),
                    }
                }
            }
        }
    }

    async fn member_contexts(
        &self,
        scope: &crate::modules::edge::domain::GatewayScope,
    ) -> Result<
        Vec<GatewayRollbackMemberSnapshotContext>,
        (Option<NodeId>, &'static str, &'static str),
    > {
        let mut contexts = Vec::with_capacity(scope.member_node_ids.len());
        for node_id in &scope.member_node_ids {
            let physical_scope = self.repository.gateway_scope(*node_id).await.map_err(|_| {
                (
                    Some(*node_id),
                    "restore",
                    "physical Gateway scope restoration failed",
                )
            })?;
            let active_routes = self.repository.active_routes(*node_id).await.map_err(|_| {
                (
                    Some(*node_id),
                    "restore",
                    "active Gateway Route restoration failed",
                )
            })?;
            let certificate_ids = active_routes
                .iter()
                .filter_map(|route| route.gateway_certificate_id)
                .collect::<BTreeSet<_>>();
            let reusable_certificate = match certificate_ids.len() {
                0 => None,
                1 => {
                    let certificate_id = *certificate_ids.iter().next().ok_or((
                        Some(*node_id),
                        "restore",
                        "active Gateway certificate identity disappeared",
                    ))?;
                    Some(
                        self.repository
                            .find_gateway_certificate(*node_id, certificate_id)
                            .await
                            .map_err(|_| {
                                (
                                    Some(*node_id),
                                    "restore",
                                    "active Gateway certificate restoration failed",
                                )
                            })?,
                    )
                }
                _ => None,
            };
            contexts.push(GatewayRollbackMemberSnapshotContext {
                scope: physical_scope,
                active_routes,
                reusable_certificate,
            });
        }
        Ok(contexts)
    }
}

fn failure(
    failed_rollout_id: GatewayRolloutId,
    node_id: Option<NodeId>,
    operation: &'static str,
    error: &'static str,
) -> GatewayRolloutRollbackReconciliationFailure {
    GatewayRolloutRollbackReconciliationFailure {
        failed_rollout_id,
        node_id,
        operation,
        error,
    }
}
