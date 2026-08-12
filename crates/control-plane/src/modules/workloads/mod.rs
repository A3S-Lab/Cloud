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
    AgentWorkloadRevisionBinding, AtomicResourceClaimReservation, CompiledResourceRequirements,
    Deployment, DeploymentPlacementGroupBinding, DeploymentReplicaBinding, DeploymentStatus,
    EffectivePlacementPolicy, ExternalBuildReference, HttpHealthCheck, ManagedOwnerKind,
    ManagedOwnerReference, McpWorkloadRevisionBinding, OciArtifact, OciArtifactReference,
    PlacementTopology, ReplicaAntiAffinity, RequestedServiceTemplate, ResourceAllocation,
    ResourceClaim, ResourceClaimBindingEvidence, ResourceClaimReleaseEvidence,
    ResourceClaimReservation, ResourceClaimState, ResourceKind, ResourceSlotBinding,
    ResourceSlotEvidence, ResourceSlotRequest, ResourceUnit, SecretBinding, SecretBindingTarget,
    ServicePort, ServiceProcess, ServiceResources, ServiceTemplate, SkillWorkloadRevisionBinding,
    Workload, WorkloadControl, WorkloadControlSpec, WorkloadDesiredState, WorkloadPlacementGroup,
    WorkloadPlacementGroupMemberPlan, WorkloadPlacementGroupMemberRole,
    WorkloadPlacementGroupState, WorkloadPlacementGroupWrite, WorkloadReplica,
    WorkloadReplicaLifecycle, WorkloadReplicaMember, WorkloadRevision, CANONICAL_REPLICA_ORDINAL,
    MAX_ATOMIC_RESOURCE_CLAIM_RESERVATIONS, MAX_WORKLOAD_PLACEMENT_GROUP_MEMBERS,
    MAX_WORKLOAD_REPLICAS,
};
pub use domain::events::{
    DeploymentCancellationRequested, DeploymentRequested, WorkloadReplicaEvacuated,
    WorkloadReplicaEvacuationRequested, WorkloadReplicaRetired, WorkloadReplicaSetReconfigured,
    WorkloadStopRequested,
};
pub use domain::repositories::{
    ActiveRuntimeTarget, CreateDeploymentBundle, DeploymentBundle,
    IDeploymentFlowWorkloadRepository, IResourceClaimRepository, ISecretRotationRestartRepository,
    IWorkloadPlacementGroupRepository, IWorkloadPlacementGroupSchedulingRepository,
    IWorkloadReplicaDeploymentRepository, IWorkloadReplicaEvacuationRepository,
    IWorkloadReplicaRetirementRepository, IWorkloadRepository, IWorkloadRuntimeTargetRepository,
    PlacementGroupCancellationWrite, PlacementGroupMaterialization, PlacementGroupMemberPlacement,
    PlacementGroupPlacement, PlacementGroupSchedulingWrite, ReconfigureReplicaSetWrite,
    ReplicaDeploymentCandidate, ReplicaDeploymentMaterialization, ReplicaEvacuationCandidate,
    ReplicaEvacuationRequest, ReplicaRetirementCompletion, ReplicaRetirementDispatch,
    ReplicaRuntimeFence, ReplicaSetWriteResult, RequestDeploymentCancellationBundle,
    RequestWorkloadStopBundle, RetiringReplicaTarget, SecretRotation, SecretRotationCompletion,
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
    InMemoryResourceClaimRepository, InMemoryWorkloadRepository, NodeDrainEvacuationFailure,
    NodeDrainEvacuationReconciler, NodeDrainEvacuationReport, OciRegistryArtifactResolver,
    PostgresResourceClaimRepository, PostgresWorkloadRepository,
    ReplicaDeploymentMaterializationFailure, ReplicaDeploymentMaterializationReport,
    ReplicaDeploymentMaterializer, ReplicaRetirementFailure, ReplicaRetirementReconciler,
    ReplicaRetirementReport, SecretRotationRestartFailure, SecretRotationRestartReconciler,
    SecretRotationRestartReport, WorkloadReconciliationFailure, WorkloadReconciliationReport,
    WorkloadRuntimeReconciler, PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_NAME,
    PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
    PREVIOUS_PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
};
pub use presentation::WorkloadsModule;
