use super::IMcpGatewaySnapshotRepository;
use crate::modules::edge::domain::services::IGatewayCommandQueue;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayScopeId, NodeId, OrganizationId, RepositoryError,
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

const EXPIRED_SNAPSHOT_FAILURE: &str =
    "MCP Gateway snapshot command expired before exact acknowledgement";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpGatewaySnapshotReconciliationFailure {
    pub organization_id: OrganizationId,
    pub gateway_scope_id: GatewayScopeId,
    pub node_id: NodeId,
    pub operation: &'static str,
    pub error: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpGatewaySnapshotReconciliationReport {
    pub pending_snapshots: usize,
    pub dispatched_commands: usize,
    pub replayed_commands: usize,
    pub unavailable_snapshots: usize,
    pub failures: Vec<McpGatewaySnapshotReconciliationFailure>,
}

pub struct McpGatewaySnapshotReconciler {
    repository: Arc<dyn IMcpGatewaySnapshotRepository>,
    commands: Arc<dyn IGatewayCommandQueue>,
    interval: Duration,
    batch_size: usize,
}

impl McpGatewaySnapshotReconciler {
    pub fn new(
        repository: Arc<dyn IMcpGatewaySnapshotRepository>,
        commands: Arc<dyn IGatewayCommandQueue>,
        interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero() || batch_size == 0 || batch_size > 10_000 {
            return Err(
                "MCP Gateway snapshot reconciliation requires a positive interval and bounded batch"
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
    ) -> Result<McpGatewaySnapshotReconciliationReport, RepositoryError> {
        let now = canonical_timestamp(now);
        let targets = self
            .repository
            .pending_mcp_gateway_snapshots(self.batch_size)
            .await?;
        let mut report = McpGatewaySnapshotReconciliationReport {
            pending_snapshots: targets.len(),
            ..McpGatewaySnapshotReconciliationReport::default()
        };
        for target in targets {
            target.validate().map_err(RepositoryError::Storage)?;
            let publication = &target.publication;
            if publication.command_issued_at > now {
                report.failures.push(failure(
                    target.organization_id,
                    target.gateway_scope_id,
                    publication.node_id,
                    "validate",
                    "MCP Gateway reconciliation time predates publication",
                ));
                continue;
            }
            if publication.command_not_after <= now {
                match self
                    .repository
                    .mark_mcp_gateway_snapshot_unavailable(
                        target.organization_id,
                        target.gateway_scope_id,
                        publication.node_id,
                        publication.revision,
                        publication.command_id,
                        EXPIRED_SNAPSHOT_FAILURE,
                        now,
                    )
                    .await
                {
                    Ok(_) => report.unavailable_snapshots += 1,
                    Err(_) => report.failures.push(failure(
                        target.organization_id,
                        target.gateway_scope_id,
                        publication.node_id,
                        "expire",
                        "MCP Gateway snapshot expiry projection failed",
                    )),
                }
                continue;
            }
            match self.commands.enqueue(publication).await {
                Ok(dispatch) => {
                    report.dispatched_commands += 1;
                    report.replayed_commands += usize::from(dispatch.replayed);
                }
                Err(_) => report.failures.push(failure(
                    target.organization_id,
                    target.gateway_scope_id,
                    publication.node_id,
                    "dispatch",
                    "MCP Gateway command dispatch failed",
                )),
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
                                    organization_id = %failure.organization_id,
                                    gateway_scope_id = %failure.gateway_scope_id,
                                    gateway_node_id = %failure.node_id,
                                    operation = failure.operation,
                                    error = failure.error,
                                    "MCP Gateway snapshot reconciliation failed"
                                );
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "MCP Gateway snapshot reconciliation scan failed"
                        ),
                    }
                }
            }
        }
    }
}

fn failure(
    organization_id: OrganizationId,
    gateway_scope_id: GatewayScopeId,
    node_id: NodeId,
    operation: &'static str,
    error: &'static str,
) -> McpGatewaySnapshotReconciliationFailure {
    McpGatewaySnapshotReconciliationFailure {
        organization_id,
        gateway_scope_id,
        node_id,
        operation,
        error,
    }
}
