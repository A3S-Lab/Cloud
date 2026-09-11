use crate::modules::shared_kernel::domain::{NodeCommandId, RepositoryError, Sha256Digest};
use crate::modules::workloads::domain::repositories::{
    RetiringReplicaTarget, WorkloadWriterFenceCommit,
};
use a3s_cloud_contracts::{NodeCommandAck, NodeCommandEnvelope};
use async_trait::async_trait;

/// Exact RuntimeRemove evidence Workloads is willing to admit into a writer
/// fence. The Fleet command journal stays behind Infrastructure; Domain only
/// receives the contract envelope plus digests needed for the receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadRuntimeRemoveEvidence {
    pub command_id: NodeCommandId,
    pub command_payload_digest: Sha256Digest,
    pub envelope: NodeCommandEnvelope,
}

/// Optional owner adapter that turns exact Runtime-removal evidence into a
/// Workloads-owned writer fence and one continuation Operation. Workloads
/// persists both atomically with its Runtime fence; the adapter owns neither
/// the replica lifecycle nor the Operation queue.
#[async_trait]
pub trait IWorkloadWriterFenceAdapter: Send + Sync {
    async fn prepare_replica_retirement(
        &self,
        target: &RetiringReplicaTarget,
        removal: &WorkloadRuntimeRemoveEvidence,
        acknowledgement: &NodeCommandAck,
    ) -> Result<Option<WorkloadWriterFenceCommit>, RepositoryError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UnrestrictedWorkloadWriterFenceAdapter;

#[async_trait]
impl IWorkloadWriterFenceAdapter for UnrestrictedWorkloadWriterFenceAdapter {
    async fn prepare_replica_retirement(
        &self,
        _target: &RetiringReplicaTarget,
        _removal: &WorkloadRuntimeRemoveEvidence,
        _acknowledgement: &NodeCommandAck,
    ) -> Result<Option<WorkloadWriterFenceCommit>, RepositoryError> {
        Ok(None)
    }
}
