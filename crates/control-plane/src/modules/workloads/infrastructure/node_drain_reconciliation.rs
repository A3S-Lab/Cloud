use crate::modules::fleet::domain::repositories::INodeDrainRepository;
use crate::modules::fleet::domain::value_objects::NodeState;
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
    pub draining_nodes: usize,
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
        let draining_nodes = self.nodes.list_draining(self.node_batch_size).await?;
        let mut report = NodeDrainEvacuationReport {
            draining_nodes: draining_nodes.len(),
            ..NodeDrainEvacuationReport::default()
        };
        for node in draining_nodes {
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
                        message: format!("scan node drain evacuation candidates: {error}"),
                    });
                    continue;
                }
            };
            report.candidates += candidates.len();
            let current = match self
                .nodes
                .find_drain_node(node.organization_id, node.id)
                .await
            {
                Ok(current) => current,
                Err(error) => {
                    report.failures.push(NodeDrainEvacuationFailure {
                        node_id: node.id,
                        workload_id: None,
                        replica_id: None,
                        message: format!("revalidate draining node: {error}"),
                    });
                    continue;
                }
            };
            if current.state != NodeState::Draining {
                report.skipped += candidates.len();
                continue;
            }
            for candidate in candidates {
                match self
                    .evacuations
                    .request_replica_evacuation(ReplicaEvacuationRequest {
                        candidate,
                        requested_at: now,
                        correlation_id: evacuation_correlation_id(candidate),
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
                                draining_nodes = report.draining_nodes,
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

fn evacuation_correlation_id(candidate: ReplicaEvacuationCandidate) -> Uuid {
    Uuid::new_v5(
        &candidate.replica_id.as_uuid(),
        format!(
            "{EVACUATION_CORRELATION_DOMAIN}:{}:{}",
            candidate.replica_generation, candidate.source_node_id
        )
        .as_bytes(),
    )
}

#[cfg(test)]
mod tests;
