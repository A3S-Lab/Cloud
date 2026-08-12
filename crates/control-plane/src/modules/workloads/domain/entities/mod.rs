mod deployment;
mod resource_allocation;
mod resource_claim;
mod resource_requirements;
mod secret_binding;
mod workload;
mod workload_control;
mod workload_replica;
mod workload_revision;

pub use deployment::{Deployment, DeploymentStatus};
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
    ReplicaAntiAffinity, WorkloadControl, WorkloadControlSpec, MAX_WORKLOAD_REPLICAS,
};
pub use workload_replica::{
    DeploymentReplicaBinding, WorkloadReplica, WorkloadReplicaLifecycle, WorkloadReplicaMember,
    CANONICAL_REPLICA_ORDINAL,
};
pub use workload_revision::{
    AgentWorkloadRevisionBinding, ExternalBuildReference, HttpHealthCheck,
    McpWorkloadRevisionBinding, OciArtifact, OciArtifactReference, RequestedServiceTemplate,
    ServicePort, ServiceProcess, ServiceResources, ServiceTemplate, SkillWorkloadRevisionBinding,
    WorkloadRevision,
};
