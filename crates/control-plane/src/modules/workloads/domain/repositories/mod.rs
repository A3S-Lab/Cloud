mod placement_group_repository;
mod placement_group_scheduling_repository;
mod resource_claim_repository;
mod workload_repository;

pub use placement_group_repository::{
    IWorkloadPlacementGroupRepository, PlacementGroupMaterialization,
};
pub use placement_group_scheduling_repository::{
    IWorkloadPlacementGroupSchedulingRepository, PlacementGroupCancellationWrite,
    PlacementGroupMemberPlacement, PlacementGroupPlacement, PlacementGroupSchedulingWrite,
};
pub use resource_claim_repository::IResourceClaimRepository;
pub(crate) use resource_claim_repository::{
    capacity_unavailable, is_capacity_unavailable, is_placement_unavailable, placement_unavailable,
};
pub use workload_repository::{
    ActiveRuntimeTarget, CreateDeploymentBundle, DeploymentBundle,
    ISecretRotationRestartRepository, IWorkloadReplicaDeploymentRepository,
    IWorkloadReplicaEvacuationRepository, IWorkloadReplicaRetirementRepository,
    IWorkloadRepository, IWorkloadRuntimeTargetRepository, IWorkloadWriterFenceRepository,
    ReconfigureReplicaSetWrite, ReplicaDeploymentCandidate, ReplicaDeploymentMaterialization,
    ReplicaEvacuationCandidate, ReplicaEvacuationRequest, ReplicaRetirementCompletion,
    ReplicaRetirementDispatch, ReplicaRuntimeFence, ReplicaSetWriteResult,
    RequestDeploymentCancellationBundle, RequestWorkloadStopBundle, RetiringReplicaTarget,
    SecretRotation, SecretRotationCompletion, SecretRotationReconciliation, WorkloadStopBundle,
    WorkloadWriterFenceCommit,
};

pub trait IDeploymentFlowWorkloadRepository:
    IWorkloadRepository
    + IWorkloadPlacementGroupRepository
    + IWorkloadPlacementGroupSchedulingRepository
{
}

impl<T> IDeploymentFlowWorkloadRepository for T where
    T: IWorkloadRepository
        + IWorkloadPlacementGroupRepository
        + IWorkloadPlacementGroupSchedulingRepository
{
}
