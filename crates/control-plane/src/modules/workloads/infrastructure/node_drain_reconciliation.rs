use crate::modules::fleet::domain::repositories::{
    INodeDrainRepository, NodeEvacuationCause, NodeEvacuationSource,
};
use crate::modules::shared_kernel::domain::{
    NodeId, RepositoryError, WorkloadId, WorkloadReplicaId,
};
use crate::modules::workloads::domain::repositories::{
    IWorkloadReplicaEvacuationRepository, ReplicaEvacuationCandidate, ReplicaEvacuationRequest,
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

const EVACUATION_CORRELATION_DOMAIN: &str = "a3s.cloud.node-drain-evacuation.v1";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeDrainEvacuationReport {
    pub source_nodes: usize,
    pub manual_drain_nodes: usize,
    pub maintenance_nodes: usize,
    pub candidates: usize,
    pub requested: usize,
    pub replayed: usize,
    pub skipped: usize,
    pub failures: Vec<NodeDrainEvacuationFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeDrainEvacuationFailure {
    pub node_id: NodeId,
    pub workload_id: Option<WorkloadId>,
    pub replica_id: Option<WorkloadReplicaId>,
    pub message: String,
}

pub struct NodeDrainEvacuationReconciler {
    nodes: Arc<dyn INodeDrainRepository>,
    evacuations: Arc<dyn IWorkloadReplicaEvacuationRepository>,
    reconcile_interval: Duration,
    node_batch_size: usize,
    replica_batch_size: usize,
}

impl NodeDrainEvacuationReconciler {
    pub fn new(
        nodes: Arc<dyn INodeDrainRepository>,
        evacuations: Arc<dyn IWorkloadReplicaEvacuationRepository>,
        reconcile_interval: Duration,
        node_batch_size: usize,
        replica_batch_size: usize,
    ) -> Result<Self, String> {
        if reconcile_interval.is_zero()
            || node_batch_size == 0
            || node_batch_size > 10_000
            || replica_batch_size == 0
            || replica_batch_size > 10_000
        {
            return Err("node drain evacuation reconciliation policy is invalid".into());
        }
        Ok(Self {
            nodes,
            evacuations,
            reconcile_interval,
            node_batch_size,
            replica_batch_size,
        })
    }

    pub async fn run_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<NodeDrainEvacuationReport, RepositoryError> {
        let sources = self
            .nodes
            .list_evacuation_sources(now, self.node_batch_size)
            .await?;
        let mut report = NodeDrainEvacuationReport {
            source_nodes: sources.len(),
            manual_drain_nodes: sources
                .iter()
                .filter(|source| source.cause == NodeEvacuationCause::ManualDrain)
                .count(),
            maintenance_nodes: sources
                .iter()
                .filter(|source| {
                    matches!(source.cause, NodeEvacuationCause::PoolMaintenance { .. })
                })
                .count(),
            ..NodeDrainEvacuationReport::default()
        };
        for source in sources {
            let node = &source.node;
            let candidates = match self
                .evacuations
                .pending_replica_evacuations(node.organization_id, node.id, self.replica_batch_size)
                .await
            {
                Ok(candidates) => candidates,
                Err(error) => {
                    report.failures.push(NodeDrainEvacuationFailure {
                        node_id: node.id,
                        workload_id: None,
                        replica_id: None,
                        message: format!("scan node evacuation candidates: {error}"),
                    });
                    continue;
                }
            };
            report.candidates += candidates.len();
            let current = match self
                .nodes
                .find_evacuation_source(node.organization_id, node.id, now)
                .await
            {
                Ok(current) => current,
                Err(RepositoryError::NotFound) => {
                    report.skipped += candidates.len();
                    continue;
                }
                Err(error) => {
                    report.failures.push(NodeDrainEvacuationFailure {
                        node_id: node.id,
                        workload_id: None,
                        replica_id: None,
                        message: format!("revalidate evacuation source: {error}"),
                    });
                    continue;
                }
            };
            if !same_source(&source, &current) {
                report.skipped += candidates.len();
                continue;
            }
            for candidate in candidates {
                match self
                    .evacuations
                    .request_replica_evacuation(ReplicaEvacuationRequest {
                        candidate,
                        requested_at: now,
                        correlation_id: evacuation_correlation_id(candidate, &source.cause),
                    })
                    .await
                {
                    Ok(result) if result.replayed => report.replayed += 1,
                    Ok(_) => report.requested += 1,
                    Err(error) => report.failures.push(NodeDrainEvacuationFailure {
                        node_id: candidate.source_node_id,
                        workload_id: Some(candidate.workload_id),
                        replica_id: Some(candidate.replica_id),
                        message: format!("request replica evacuation: {error}"),
                    }),
                }
            }
        }
        Ok(report)
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.reconcile_interval);
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
                            for failure in &report.failures {
                                tracing::warn!(
                                    node_id = %failure.node_id,
                                    workload_id = ?failure.workload_id,
                                    replica_id = ?failure.replica_id,
                                    error = %failure.message,
                                    "node drain evacuation reconciliation failed"
                                );
                            }
                            tracing::debug!(
                                source_nodes = report.source_nodes,
                                manual_drain_nodes = report.manual_drain_nodes,
                                maintenance_nodes = report.maintenance_nodes,
                                candidates = report.candidates,
                                requested = report.requested,
                                replayed = report.replayed,
                                skipped = report.skipped,
                                failures = report.failures.len(),
                                "node drain evacuation reconciliation cycle completed"
                            );
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "draining node scan failed"
                        ),
                    }
                }
            }
        }
    }
}

fn same_source(expected: &NodeEvacuationSource, current: &NodeEvacuationSource) -> bool {
    expected.node.organization_id == current.node.organization_id
        && expected.node.id == current.node.id
        && expected.cause == current.cause
}

fn evacuation_correlation_id(
    candidate: ReplicaEvacuationCandidate,
    cause: &NodeEvacuationCause,
) -> Uuid {
    let identity = match cause {
        NodeEvacuationCause::ManualDrain => format!(
            "{EVACUATION_CORRELATION_DOMAIN}:{}:{}",
            candidate.replica_generation, candidate.source_node_id
        ),
        NodeEvacuationCause::PoolMaintenance {
            pool_id,
            generation,
            ..
        } => format!(
            "{EVACUATION_CORRELATION_DOMAIN}:{}:{}:maintenance:{pool_id}:{generation}",
            candidate.replica_generation, candidate.source_node_id
        ),
    };
    Uuid::new_v5(&candidate.replica_id.as_uuid(), identity.as_bytes())
}

#[cfg(test)]
mod tests;
