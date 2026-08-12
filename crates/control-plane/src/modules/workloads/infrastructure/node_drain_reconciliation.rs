use crate::modules::fleet::domain::events::{NodePoolChangeKind, NodePoolChanged};
use crate::modules::fleet::domain::repositories::{
    INodeDrainRepository, INodePoolRepository, NodeEvacuationCause, NodeEvacuationSource,
    NodePoolWrite,
};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, NodeId, NodePoolId, RepositoryError, WorkloadId, WorkloadReplicaId,
};
use crate::modules::workloads::domain::repositories::{
    IResourceClaimRepository, IWorkloadReplicaEvacuationRepository, ReplicaEvacuationCandidate,
    ReplicaEvacuationRequest,
};
use chrono::{DateTime, Utc};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

const EVACUATION_CORRELATION_DOMAIN: &str = "a3s.cloud.node-drain-evacuation.v1";
const MEMBER_REMOVAL_COMPLETION_DOMAIN: &str = "a3s.cloud.node-pool-member-removal-completion.v1";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeDrainEvacuationReport {
    pub source_nodes: usize,
    pub manual_drain_nodes: usize,
    pub maintenance_nodes: usize,
    pub member_removal_nodes: usize,
    pub member_removals_completed: usize,
    pub member_removal_completion_replayed: usize,
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
    node_pools: Arc<dyn INodePoolRepository>,
    evacuations: Arc<dyn IWorkloadReplicaEvacuationRepository>,
    resource_claims: Arc<dyn IResourceClaimRepository>,
    reconcile_interval: Duration,
    node_batch_size: usize,
    replica_batch_size: usize,
}

impl NodeDrainEvacuationReconciler {
    pub fn new(
        nodes: Arc<dyn INodeDrainRepository>,
        node_pools: Arc<dyn INodePoolRepository>,
        evacuations: Arc<dyn IWorkloadReplicaEvacuationRepository>,
        resource_claims: Arc<dyn IResourceClaimRepository>,
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
            node_pools,
            evacuations,
            resource_claims,
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
            member_removal_nodes: sources
                .iter()
                .filter(|source| {
                    matches!(source.cause, NodeEvacuationCause::PoolMemberRemoval { .. })
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
            if let NodeEvacuationCause::PoolMemberRemoval {
                pool_id,
                generation,
            } = &source.cause
            {
                match self
                    .complete_member_removal(&source, *pool_id, *generation, now)
                    .await
                {
                    Ok(Some(true)) => report.member_removal_completion_replayed += 1,
                    Ok(Some(false)) => report.member_removals_completed += 1,
                    Ok(None) => {}
                    Err(error) => report.failures.push(NodeDrainEvacuationFailure {
                        node_id: node.id,
                        workload_id: None,
                        replica_id: None,
                        message: format!("complete node pool member removal: {error}"),
                    }),
                }
            }
        }
        Ok(report)
    }

    async fn complete_member_removal(
        &self,
        source: &NodeEvacuationSource,
        pool_id: NodePoolId,
        generation: u64,
        completed_at: DateTime<Utc>,
    ) -> Result<Option<bool>, RepositoryError> {
        let node = &source.node;
        if self
            .evacuations
            .has_replica_placements(node.organization_id, node.id)
            .await?
            || self
                .resource_claims
                .has_active_claims(node.organization_id, node.id)
                .await?
        {
            return Ok(None);
        }
        let current = match self
            .nodes
            .find_evacuation_source(node.organization_id, node.id, completed_at)
            .await
        {
            Ok(current) => current,
            Err(RepositoryError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        if !same_source(source, &current) {
            return Ok(None);
        }
        let mut pool = self.node_pools.find(node.organization_id, pool_id).await?;
        if pool
            .member_removal(node.id)
            .is_none_or(|removal| removal.generation != generation)
        {
            return Ok(None);
        }
        let expected_version = pool.aggregate_version;
        pool.complete_member_removal(node.id, generation, completed_at)
            .map_err(RepositoryError::Conflict)?;
        let canonical = serde_json::to_vec(&json!({
            "action": "completeMemberRemoval",
            "organizationId": node.organization_id,
            "nodePoolId": pool_id,
            "nodeId": node.id,
            "generation": generation,
        }))
        .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/node-pools/{pool_id}/members/removal/completion",
                node.organization_id
            ),
            format!("{}:{generation}", node.id),
            &canonical,
        )
        .map_err(RepositoryError::Conflict)?;
        let correlation_id = Uuid::new_v5(
            &pool_id.as_uuid(),
            format!(
                "{MEMBER_REMOVAL_COMPLETION_DOMAIN}:{}:{generation}",
                node.id
            )
            .as_bytes(),
        );
        let event = NodePoolChanged::envelope(
            &pool,
            NodePoolChangeKind::MembersRemoved,
            completed_at,
            correlation_id,
        )
        .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        self.node_pools
            .save(NodePoolWrite {
                pool,
                expected_version: Some(expected_version),
                event,
                idempotency,
            })
            .await
            .map(|write| Some(write.replayed))
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
                                member_removal_nodes = report.member_removal_nodes,
                                member_removals_completed = report.member_removals_completed,
                                member_removal_completion_replayed = report.member_removal_completion_replayed,
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
        NodeEvacuationCause::PoolMemberRemoval {
            pool_id,
            generation,
        } => format!(
            "{EVACUATION_CORRELATION_DOMAIN}:{}:{}:member-removal:{pool_id}:{generation}",
            candidate.replica_generation, candidate.source_node_id
        ),
    };
    Uuid::new_v5(&candidate.replica_id.as_uuid(), identity.as_bytes())
}

#[cfg(test)]
mod tests;
