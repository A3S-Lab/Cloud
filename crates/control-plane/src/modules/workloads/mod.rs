pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    CancelDeployment, CancelDeploymentHandler, CancelDeploymentResult,
    CreateAgentWorkloadDeployment, CreateAgentWorkloadDeploymentHandler,
    CreateSourceWorkloadDeployment, CreateSourceWorkloadDeploymentHandler,
    CreateSourceWorkloadDeploymentResult, CreateWorkloadDeployment,
    CreateWorkloadDeploymentHandler, CreateWorkloadDeploymentResult, DeploymentQueryResult,
    GetDeployment, GetDeploymentHandler, GetWorkload, GetWorkloadHandler, GetWorkloadLogs,
    GetWorkloadLogsHandler, ListWorkloads, ListWorkloadsHandler, RollbackWorkloadDeployment,
    RollbackWorkloadDeploymentHandler, RollbackWorkloadDeploymentResult, SourceWorkloadTemplate,
    StopWorkload, StopWorkloadHandler, StopWorkloadResult, UpdateAgentWorkloadDeployment,
    UpdateAgentWorkloadDeploymentHandler, UpdateWorkloadDeployment,
    UpdateWorkloadDeploymentHandler, UpdateWorkloadDeploymentResult, WorkloadLogGapReason,
    WorkloadLogPage, WorkloadLogRecord, WorkloadQueryResult, WorkloadReplicaQueryResult,
};
pub use domain::entities::{
    AgentWorkloadRevisionBinding, CompiledResourceRequirements, Deployment,
    DeploymentReplicaBinding, DeploymentStatus, EffectivePlacementPolicy, ExternalBuildReference,
    HttpHealthCheck, ManagedOwnerKind, ManagedOwnerReference, McpWorkloadRevisionBinding,
    OciArtifact, OciArtifactReference, PlacementTopology, RequestedServiceTemplate,
    ResourceAllocation, ResourceClaim, ResourceClaimBindingEvidence, ResourceClaimReleaseEvidence,
    ResourceClaimReservation, ResourceClaimState, ResourceKind, ResourceSlotBinding,
    ResourceSlotEvidence, ResourceSlotRequest, ResourceUnit, SecretBinding, SecretBindingTarget,
    ServicePort, ServiceProcess, ServiceResources, ServiceTemplate, SkillWorkloadRevisionBinding,
    Workload, WorkloadControl, WorkloadControlSpec, WorkloadDesiredState, WorkloadReplica,
    WorkloadReplicaMember, WorkloadRevision, CANONICAL_REPLICA_ORDINAL,
};
pub use domain::events::{
    DeploymentCancellationRequested, DeploymentRequested, WorkloadStopRequested,
};
pub use domain::repositories::{
    ActiveRuntimeTarget, CreateDeploymentBundle, DeploymentBundle, IResourceClaimRepository,
    ISecretRotationRestartRepository, IWorkloadRepository, IWorkloadRuntimeTargetRepository,
    RequestDeploymentCancellationBundle, RequestWorkloadStopBundle, SecretRotation,
    SecretRotationCompletion, SecretRotationReconciliation, WorkloadStopBundle,
};
pub use domain::services::{
    DeploymentGatewayPublication, DeploymentRouteObservation, DeploymentRouteStage,
    DeploymentRouteUpdateRequest, IDeploymentRouteUpdater, IOciArtifactResolver,
    OciArtifactResolutionError, OciRegistryCredentialReference, UnroutedDeploymentRouteUpdater,
};
pub use infrastructure::{
    DeploymentFlowConfig, DeploymentFlowDependencies, DeploymentFlowRuntime,
    IWorkloadRuntimeControl, InMemoryResourceClaimRepository, InMemoryWorkloadRepository,
    OciRegistryArtifactResolver, PostgresResourceClaimRepository, PostgresWorkloadRepository,
    SecretRotationRestartFailure, SecretRotationRestartReconciler, SecretRotationRestartReport,
    WorkloadReconciliationFailure, WorkloadReconciliationReport, WorkloadRuntimeReconciler,
};
pub use presentation::WorkloadsModule;
