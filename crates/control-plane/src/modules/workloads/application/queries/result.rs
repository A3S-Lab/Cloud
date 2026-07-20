use crate::modules::fleet::domain::repositories::{NodeLogChunkMetadata, RuntimeObservationRecord};
use crate::modules::operations::domain::entities::OperationProjection;
use crate::modules::shared_kernel::domain::{NodeId, WorkloadId, WorkloadRevisionId};
use crate::modules::workloads::domain::entities::{Deployment, Workload, WorkloadRevision};
use a3s_runtime::contract::RuntimeLogChunk;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentQueryResult {
    pub deployment: Deployment,
    pub revision: WorkloadRevision,
    pub operation: Option<OperationProjection>,
    pub observation: Option<RuntimeObservationRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadQueryResult {
    pub workload: Workload,
    pub revisions: Vec<WorkloadRevision>,
    pub deployments: Vec<DeploymentQueryResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadLogGapReason {
    Missing,
    Corrupt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadLogRecord {
    Data(RuntimeLogChunk),
    Gap {
        metadata: NodeLogChunkMetadata,
        reason: WorkloadLogGapReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadLogPage {
    pub workload_id: WorkloadId,
    pub revision_id: WorkloadRevisionId,
    pub node_id: Option<NodeId>,
    pub unit_id: String,
    pub generation: u64,
    pub records: Vec<WorkloadLogRecord>,
    pub next_after_sequence: Option<u64>,
}

impl WorkloadQueryResult {
    pub fn desired_revision(&self) -> Option<&WorkloadRevision> {
        self.revisions.first()
    }

    pub fn active_revision(&self) -> Option<&WorkloadRevision> {
        let active_revision_id = self.workload.active_revision_id?;
        self.revisions
            .iter()
            .find(|revision| revision.id == active_revision_id)
    }

    pub fn latest_deployment(&self) -> Option<&DeploymentQueryResult> {
        self.deployments.first()
    }
}
