mod deployment;
mod placement_group;
mod resource_allocation;
mod resource_claim;
mod resource_requirements;
mod secret_binding;
mod workload;
mod workload_control;
mod workload_replica;
mod workload_revision;
mod workload_writer_fence;

pub use deployment::{Deployment, DeploymentStatus};
pub use placement_group::{
    DeploymentPlacementGroupBinding, WorkloadPlacementGroup, WorkloadPlacementGroupMemberPlan,
    WorkloadPlacementGroupMemberRole, WorkloadPlacementGroupState, WorkloadPlacementGroupWrite,
};
pub use resource_allocation::{
    ResourceAllocation, ResourceKind, ResourceSlotBinding, ResourceSlotEvidence,
    ResourceSlotRequest, ResourceUnit,
};
pub use resource_claim::{
    AtomicResourceClaimReservation, ResourceClaim, ResourceClaimBindingEvidence,
    ResourceClaimReleaseEvidence, ResourceClaimReservation, ResourceClaimState,
    MAX_ATOMIC_RESOURCE_CLAIM_RESERVATIONS,
};
pub use resource_requirements::CompiledResourceRequirements;
pub use secret_binding::{SecretBinding, SecretBindingTarget};
pub use workload::{Workload, WorkloadDesiredState};
pub use workload_control::{
    EffectivePlacementPolicy, ManagedOwnerKind, ManagedOwnerReference, PlacementTopology,
    ReplicaAntiAffinity, WorkloadControl, WorkloadControlSpec,
    MAX_WORKLOAD_PLACEMENT_GROUP_MEMBERS, MAX_WORKLOAD_REPLICAS,
};
pub use workload_replica::{
    DeploymentReplicaBinding, WorkloadReplica, WorkloadReplicaLifecycle, WorkloadReplicaMember,
    CANONICAL_REPLICA_ORDINAL,
};
pub use workload_revision::{
    AgentReleaseAdmission, AgentWorkloadRevisionBinding, ExternalBuildReference, HttpHealthCheck,
    McpWorkloadRevisionBinding, OciArtifact, OciArtifactReference, RequestedServiceTemplate,
    ServicePort, ServiceProcess, ServiceResources, ServiceTemplate, SkillWorkloadRevisionBinding,
    WorkloadRevision,
};
pub use workload_writer_fence::{
    WorkloadWriterFenceReceipt, WorkloadWriterFenceReceiptSpec,
    WORKLOAD_WRITER_FENCE_RECEIPT_SCHEMA,
};
