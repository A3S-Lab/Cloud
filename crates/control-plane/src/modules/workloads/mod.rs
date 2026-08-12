pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    BindSkillWorkloadDeployment, BindSkillWorkloadDeploymentHandler, CancelDeployment,
    CancelDeploymentHandler, CancelDeploymentResult, CreateAgentWorkloadDeployment,
    CreateAgentWorkloadDeploymentHandler, CreateSourceWorkloadDeployment,
    CreateSourceWorkloadDeploymentHandler, CreateSourceWorkloadDeploymentResult,
    CreateWorkloadDeployment, CreateWorkloadDeploymentHandler, CreateWorkloadDeploymentResult,
    DeploymentQueryResult, GetDeployment, GetDeploymentHandler, GetWorkload, GetWorkloadHandler,
    GetWorkloadLogs, GetWorkloadLogsHandler, ListWorkloads, ListWorkloadsHandler,
    RollbackWorkloadDeployment, RollbackWorkloadDeploymentHandler,
    RollbackWorkloadDeploymentResult, SourceWorkloadTemplate, StopWorkload, StopWorkloadHandler,
    StopWorkloadResult, UnbindSkillWorkloadDeployment, UnbindSkillWorkloadDeploymentHandler,
    UpdateAgentWorkloadDeployment, UpdateAgentWorkloadDeploymentHandler, UpdateWorkloadDeployment,
    UpdateWorkloadDeploymentHandler, UpdateWorkloadDeploymentResult, WorkloadLogGapReason,
    WorkloadLogPage, WorkloadLogRecord, WorkloadQueryResult, WorkloadReplicaQueryResult,
};
pub use domain::entities::{
    AgentWorkloadRevisionBinding, CompiledResourceRequirements, Deployment,
    DeploymentReplicaBinding, DeploymentStatus, EffectivePlacementPolicy, ExternalBuildReference,
    HttpHealthCheck, ManagedOwnerKind, ManagedOwnerReference, McpWorkloadRevisionBinding,
    OciArtifact, OciArtifactReference, PlacementTopology, ReplicaAntiAffinity,
    RequestedServiceTemplate, ResourceAllocation, ResourceClaim, ResourceClaimBindingEvidence,
    ResourceClaimReleaseEvidence, ResourceClaimReservation, ResourceClaimState, ResourceKind,
    ResourceSlotBinding, ResourceSlotEvidence, ResourceSlotRequest, ResourceUnit, SecretBinding,
    SecretBindingTarget, ServicePort, ServiceProcess, ServiceResources, ServiceTemplate,
    SkillWorkloadRevisionBinding, Workload, WorkloadControl, WorkloadControlSpec,
    WorkloadDesiredState, WorkloadReplica, WorkloadReplicaLifecycle, WorkloadReplicaMember,
    WorkloadRevision, CANONICAL_REPLICA_ORDINAL, MAX_WORKLOAD_REPLICAS,
};
pub use domain::events::{
    DeploymentCancellationRequested, DeploymentRequested, WorkloadReplicaSetReconfigured,
    WorkloadStopRequested,
};
pub use domain::repositories::{
    ActiveRuntimeTarget, CreateDeploymentBundle, DeploymentBundle, IResourceClaimRepository,
    ISecretRotationRestartRepository, IWorkloadReplicaDeploymentRepository, IWorkloadRepository,
    IWorkloadRuntimeTargetRepository, ReconfigureReplicaSetWrite, ReplicaDeploymentCandidate,
    ReplicaDeploymentMaterialization, ReplicaSetWriteResult, RequestDeploymentCancellationBundle,
    RequestWorkloadStopBundle, SecretRotation, SecretRotationCompletion,
    SecretRotationReconciliation, WorkloadStopBundle,
};
pub use domain::services::{
    DeploymentGatewayPublication, DeploymentRouteObservation, DeploymentRouteStage,
    DeploymentRouteUpdateRequest, IDeploymentRouteUpdater, IOciArtifactResolver,
    OciArtifactResolutionError, OciRegistryCredentialReference, ReplicaSetReconfiguration,
    ReplicaSetReconfigurationError, UnroutedDeploymentRouteUpdater,
};
pub use infrastructure::{
    project_replica_runtime_spec, project_runtime_spec, DeploymentFlowConfig,
    DeploymentFlowDependencies, DeploymentFlowRuntime, IWorkloadRuntimeControl,
    InMemoryResourceClaimRepository, InMemoryWorkloadRepository, OciRegistryArtifactResolver,
    PostgresResourceClaimRepository, PostgresWorkloadRepository,
    ReplicaDeploymentMaterializationFailure, ReplicaDeploymentMaterializationReport,
    ReplicaDeploymentMaterializer, SecretRotationRestartFailure, SecretRotationRestartReconciler,
    SecretRotationRestartReport, WorkloadReconciliationFailure, WorkloadReconciliationReport,
    WorkloadRuntimeReconciler,
};
pub use presentation::WorkloadsModule;
