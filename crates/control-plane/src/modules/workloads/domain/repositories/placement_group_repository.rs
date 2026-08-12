use crate::modules::shared_kernel::domain::{
    OrganizationId, RepositoryError, WorkloadPlacementGroupId, WorkloadReplicaId,
};
use crate::modules::workloads::domain::entities::{
    WorkloadPlacementGroup, WorkloadPlacementGroupWrite, WorkloadReplicaMember,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementGroupMaterialization {
    pub group: WorkloadPlacementGroup,
    pub replica_members: Vec<WorkloadReplicaMember>,
    pub replayed: bool,
}

#[async_trait]
pub trait IWorkloadPlacementGroupRepository: Send + Sync {
    async fn materialize_placement_group(
        &self,
        write: WorkloadPlacementGroupWrite,
    ) -> Result<PlacementGroupMaterialization, RepositoryError>;

    async fn find_placement_group(
        &self,
        organization_id: OrganizationId,
        group_id: WorkloadPlacementGroupId,
    ) -> Result<WorkloadPlacementGroup, RepositoryError>;

    async fn find_placement_group_for_replica_generation(
        &self,
        organization_id: OrganizationId,
        replica_id: WorkloadReplicaId,
        replica_generation: u64,
    ) -> Result<WorkloadPlacementGroup, RepositoryError>;
}
