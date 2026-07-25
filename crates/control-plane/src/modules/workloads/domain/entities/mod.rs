mod deployment;
mod resource_allocation;
mod resource_claim;
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
    ResourceClaim, ResourceClaimBindingEvidence, ResourceClaimReleaseEvidence,
    ResourceClaimReservation, ResourceClaimState,
};
pub use secret_binding::{SecretBinding, SecretBindingTarget};
pub use workload::{Workload, WorkloadDesiredState};
pub use workload_control::{
    EffectivePlacementPolicy, ManagedOwnerKind, ManagedOwnerReference, PlacementTopology,
    WorkloadControl, WorkloadControlSpec,
};
pub use workload_replica::{
    DeploymentReplicaBinding, WorkloadReplica, WorkloadReplicaMember, CANONICAL_REPLICA_ORDINAL,
};
pub use workload_revision::{
    ExternalBuildReference, HttpHealthCheck, OciArtifact, OciArtifactReference,
    RequestedServiceTemplate, ServicePort, ServiceProcess, ServiceResources, ServiceTemplate,
    WorkloadRevision,
};
