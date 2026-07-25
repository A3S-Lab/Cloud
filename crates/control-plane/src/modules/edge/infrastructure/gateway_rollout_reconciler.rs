use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::services::IGatewayCommandQueue;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayRolloutId, NodeId, RepositoryError,
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

const EXPIRED_REPLICA_FAILURE: &str =
    "Gateway rollout command expired before exact acknowledgement";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRolloutReconciliationFailure {
    pub rollout_id: GatewayRolloutId,
    pub node_id: NodeId,
    pub operation: &'static str,
    pub error: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GatewayRolloutReconciliationReport {
    pub active_rollouts: usize,
    pub pending_replicas: usize,
    pub dispatched_commands: usize,
    pub replayed_commands: usize,
    pub expired_replicas: usize,
    pub failures: Vec<GatewayRolloutReconciliationFailure>,
}

pub struct GatewayRolloutReconciler {
    repository: Arc<dyn IEdgeRepository>,
    commands: Arc<dyn IGatewayCommandQueue>,
    interval: Duration,
    batch_size: usize,
}

impl GatewayRolloutReconciler {
    pub fn new(
        repository: Arc<dyn IEdgeRepository>,
        commands: Arc<dyn IGatewayCommandQueue>,
        interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero() || batch_size == 0 || batch_size > 10_000 {
            return Err(
                "Gateway rollout reconciliation requires a positive interval and bounded batch"
                    .into(),
            );
        }
        Ok(Self {
            repository,
            commands,
            interval,
            batch_size,
        })
    }

    pub async fn run_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<GatewayRolloutReconciliationReport, RepositoryError> {
        let now = canonical_timestamp(now);
        let targets = self
            .repository
            .pending_gateway_rollout_dispatches(self.batch_size)
            .await?;
        let mut report = GatewayRolloutReconciliationReport {
            active_rollouts: targets.len(),
            ..GatewayRolloutReconciliationReport::default()
        };
        for mut target in targets {
            target.validate().map_err(RepositoryError::Storage)?;
            report.pending_replicas += target.publications.len();
            for publication in target.publications {
                if publication.command_issued_at > now {
                    report.failures.push(failure(
                        target.rollout.id,
                        publication.node_id,
                        "validate",
                        "Gateway rollout reconciliation time predates publication",
                    ));
                    continue;
                }
                if publication.command_not_after <= now {
                    match self
                        .repository
                        .mark_gateway_rollout_replica_unavailable(
                            target.organization_id,
                            target.rollout.id,
                            publication.node_id,
                            target.rollout.aggregate_version,
                            EXPIRED_REPLICA_FAILURE,
                            now,
                        )
                        .await
                    {
                        Ok(rollout) => {
                            target.rollout = rollout;
                            report.expired_replicas += 1;
                        }
                        Err(_) => {
                            report.failures.push(failure(
                                target.rollout.id,
                                publication.node_id,
                                "expire",
                                "Gateway rollout expiry projection failed",
                            ));
                            break;
                        }
                    }
                    continue;
                }
                match self.commands.enqueue(&publication).await {
                    Ok(dispatch) => {
                        report.dispatched_commands += 1;
                        report.replayed_commands += usize::from(dispatch.replayed);
                    }
                    Err(_) => report.failures.push(failure(
                        target.rollout.id,
                        publication.node_id,
                        "dispatch",
                        "Gateway rollout command dispatch failed",
                    )),
                }
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
                                    gateway_rollout_id = %failure.rollout_id,
                                    gateway_node_id = %failure.node_id,
                                    operation = failure.operation,
                                    error = failure.error,
                                    "Gateway rollout reconciliation failed"
                                );
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "Gateway rollout reconciliation scan failed"
                        ),
                    }
                }
            }
        }
    }
}

fn failure(
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    operation: &'static str,
    error: &'static str,
) -> GatewayRolloutReconciliationFailure {
    GatewayRolloutReconciliationFailure {
        rollout_id,
        node_id,
        operation,
        error,
    }
}
