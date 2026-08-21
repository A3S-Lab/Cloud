use crate::modules::fleet::domain::entities::NodeCommand;
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::domain::repositories::{
    RetiringReplicaTarget, WorkloadWriterFenceCommit,
};
use a3s_cloud_contracts::NodeCommandAck;
use async_trait::async_trait;

/// Optional owner adapter that turns exact Fleet Runtime-removal evidence into
/// a Workloads-owned writer fence and one continuation Operation. Workloads
/// persists both atomically with its Runtime fence; the adapter owns neither
/// the replica lifecycle nor the Operation queue.
#[async_trait]
pub trait IWorkloadWriterFenceAdapter: Send + Sync {
    async fn prepare_replica_retirement(
        &self,
        target: &RetiringReplicaTarget,
        command: &NodeCommand,
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
        _command: &NodeCommand,
        _acknowledgement: &NodeCommandAck,
    ) -> Result<Option<WorkloadWriterFenceCommit>, RepositoryError> {
        Ok(None)
    }
}
