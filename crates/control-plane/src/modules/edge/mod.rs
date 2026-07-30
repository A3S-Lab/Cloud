pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    CreateDomainClaim, CreateDomainClaimHandler, CreateDomainClaimResult, CreateGatewayScope,
    CreateGatewayScopeHandler, CreateGatewayScopeResult, GetDomainClaim, GetDomainClaimHandler,
    GetRoute, GetRouteHandler, ListDomainClaims, ListDomainClaimsHandler, ListGatewayCertificates,
    ListGatewayCertificatesHandler, ListGatewayScopes, ListGatewayScopesHandler, ListRoutes,
    ListRoutesHandler, PublishRoute, PublishRouteHandler, PublishRouteResult, RevokeDomainClaim,
    RevokeDomainClaimHandler, RevokeDomainClaimResult, SignGatewayCertificate,
    SignGatewayCertificateHandler, VerifyDomainClaim, VerifyDomainClaimHandler,
    VerifyDomainClaimResult,
};
pub use domain::repositories::{
    CreateDomainClaimWrite, CreateGatewayScopeWrite, EdgeRoutePublicationResult,
    GatewayCertificateConvergenceResult, GatewayCertificateConvergenceTarget,
    GatewayCertificateRouteStatus, GatewayReplicaRecoveryTarget, GatewayRolloutDispatchTarget,
    GatewayRolloutResult, GatewayRolloutRollbackResult, GatewayRolloutRollbackTarget,
    GatewayRouteCutoverResult, IEdgeRepository, IMcpCredentialLifecycleRepository,
    IMcpCredentialRepository, IMcpRoutePolicyRepository, McpCredentialLifecycleResult,
    StageGatewayCertificateConvergence, StageGatewayRollout, StageGatewayRolloutRollback,
    StageGatewayRouteCutover, StoreMcpCredentialLifecycle, TransitionDomainClaim,
    MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY, MAX_MCP_CREDENTIAL_DELIVERY_PURGE_BATCH,
};
pub use domain::services::{
    DomainOwnershipVerificationError, DomainOwnershipVerificationRequest,
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, GatewayObservationCommand,
    GatewayObservationCommandOutcome, GatewayObservationDispatch, IDomainOwnershipVerifier,
    IGatewayCertificateAuthority, IGatewayCommandQueue, IGatewayObservationQueue,
    IMcpRouteProjectionInputReader, IRouteTargetReader, ResolvedMcpRouteProjectionInput,
    ResolvedRouteTarget, ResolvedRouteTargetSet,
};
pub use domain::{
    DomainClaim, DomainClaimState, DomainNamePattern, GatewayCertificate,
    GatewayCertificateConvergence, GatewayCertificateConvergenceReason,
    GatewayCertificateConvergenceState, GatewayCertificateMaterial, GatewayCertificateState,
    GatewayPublication, GatewayPublicationState, GatewayReplicaRecovery,
    GatewayReplicaRecoveryState, GatewayReplicaRollout, GatewayReplicaRolloutState, GatewayRollout,
    GatewayRolloutPolicy, GatewayRolloutRollback, GatewayRolloutRollbackState, GatewayRolloutState,
    GatewayRouteCutover, GatewayRouteCutoverState, GatewayRouteVersion, GatewayScope,
    GatewayScopeState, McpCredential, McpCredentialDelivery, McpRoutePolicy, McpRoutePolicySpec,
    Route, RouteHostname, RoutePath, RoutePortName, RouteState, RouteTarget, UpstreamEndpoint,
    MAX_MCP_CREDENTIAL_DELIVERY_TTL,
};
pub use infrastructure::persistence::{InMemoryEdgeRepository, PostgresEdgeRepository};
pub use infrastructure::{
    CompileManagedGatewayCertificateConvergenceSnapshot, CompileManagedGatewayRetainedSnapshot,
    CompileManagedGatewayRolloutRollback, CompileManagedGatewayRouteRollout,
    CompileManagedGatewayRouteSnapshot, CompileMcpGatewaySnapshot, CompiledMcpGatewaySnapshot,
    DnsDomainOwnershipVerifier, EdgeDeploymentRouteUpdater, EdgeGatewayAcknowledgementProjector,
    FleetGatewayCommandQueue, FleetGatewayObservationQueue, GatewayCertificateReconciler,
    GatewayCertificateReconciliationFailure, GatewayCertificateReconciliationReport,
    GatewayDomainClaimVersion, GatewayManagedSnapshotComposition, GatewayNodeDesiredStatePlanner,
    GatewayReplicaRecoveryReconciler, GatewayReplicaRecoveryReconciliationFailure,
    GatewayReplicaRecoveryReconciliationReport, GatewayRolloutReconciler,
    GatewayRolloutReconciliationFailure, GatewayRolloutReconciliationReport,
    GatewayRolloutRollbackCompiler, GatewayRolloutRollbackReconciler,
    GatewayRolloutRollbackReconciliationFailure, GatewayRolloutRollbackReconciliationReport,
    GatewayRouteRolloutCompiler, GatewayRouteRolloutPlanner, GatewaySnapshotCompiler,
    GatewaySnapshotCompilerConfig, GatewaySnapshotMetadata, GatewaySnapshotPublicationOwner,
    GatewaySnapshotRouteInput, IMcpGatewayProjectionSetPlanner, IMcpGatewaySnapshotRepository,
    IssuedMcpCredential, LocalDomainOwnershipVerifier, LocalGatewayCertificateAuthority,
    ManagedGatewayRollbackMemberSnapshotContext, McpCredentialIssuanceError,
    McpCredentialIssueRequest, McpCredentialIssuer, McpCredentialMaterial,
    McpCredentialMaterialGenerator, McpCredentialProjectionVersion,
    McpCredentialSuppressionVersion, McpGatewayDesiredStateReconciler,
    McpGatewayDesiredStateReconciliationFailure, McpGatewayDesiredStateReconciliationReport,
    McpGatewayIngressRoute, McpGatewayNodeProjectionAssembler, McpGatewayProjectionAssembler,
    McpGatewayProjectionCompiler, McpGatewayProjectionPlanner, McpGatewayProjectionSetPlanner,
    McpGatewayReconciliationScope, McpGatewaySnapshotAnchor, McpGatewaySnapshotDispatchTarget,
    McpGatewaySnapshotInputs, McpGatewaySnapshotReconciler,
    McpGatewaySnapshotReconciliationFailure, McpGatewaySnapshotReconciliationReport,
    McpGatewaySnapshotReconciliationState, McpGatewaySnapshotStageResult, McpGatewaySnapshotStatus,
    McpRouteProjectionInputReader, McpRouteProjectionPlanner, McpRouteProjectionVersion,
    McpRouteTargetCandidate, McpRouteTargetProjectionCompiler, PlanGatewayNodeDesiredState,
    PlanGatewayRouteRollout, PlanManagedGatewayRouteRollout, PlanMcpGatewayProjectionSet,
    PlanMcpRouteProjection, PlannedGatewayNodeDesiredState, PlannedMcpGatewayNodeProjection,
    PlannedMcpGatewayProjection, PlannedMcpGatewayProjectionSet,
    StageManagedGatewayCertificateConvergence, StageManagedGatewayRollout,
    StageManagedGatewayRolloutRollback, StageManagedGatewayRouteCutover,
    StageManagedRoutePublication, StageMcpGatewaySnapshot, VaultGatewayCertificateAuthority,
    WorkloadRouteTargetReader,
};
pub use presentation::EdgeModule;
