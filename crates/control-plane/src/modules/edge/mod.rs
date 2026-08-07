pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    CreateDomainClaim, CreateDomainClaimHandler, CreateDomainClaimResult, CreateGatewayScope,
    CreateGatewayScopeHandler, CreateGatewayScopeResult, CreateMcpCredential,
    CreateMcpCredentialHandler, CreateMcpRoutePolicy, CreateMcpRoutePolicyHandler, GetDomainClaim,
    GetDomainClaimHandler, GetMcpCredential, GetMcpCredentialHandler, GetMcpRoutePolicy,
    GetMcpRoutePolicyHandler, GetRoute, GetRouteHandler, ListDomainClaims, ListDomainClaimsHandler,
    ListGatewayCertificates, ListGatewayCertificatesHandler, ListGatewayScopes,
    ListGatewayScopesHandler, ListMcpCredentials, ListMcpCredentialsHandler, ListMcpRoutePolicies,
    ListMcpRoutePoliciesHandler, ListRoutes, ListRoutesHandler,
    McpCredentialDeliveryReceiptSweeper, McpCredentialDeliveryResult, McpCredentialMutationResult,
    McpRoutePolicyApplicationService, PublishRoute, PublishRouteHandler, PublishRouteResult,
    ReviseMcpRoutePolicy, ReviseMcpRoutePolicyHandler, RevokeDomainClaim, RevokeDomainClaimHandler,
    RevokeDomainClaimResult, RevokeMcpCredential, RevokeMcpCredentialHandler, RotateMcpCredential,
    RotateMcpCredentialHandler, SignGatewayCertificate, SignGatewayCertificateHandler,
    VerifyDomainClaim, VerifyDomainClaimHandler, VerifyDomainClaimResult,
};
pub use domain::repositories::{
    CreateDomainClaimWrite, CreateGatewayScopeWrite, EdgeRoutePublicationResult,
    GatewayCertificateConvergenceResult, GatewayCertificateConvergenceTarget,
    GatewayCertificateRouteStatus, GatewayReplicaRecoveryTarget, GatewayRolloutDispatchTarget,
    GatewayRolloutResult, GatewayRolloutRollbackResult, GatewayRolloutRollbackTarget,
    GatewayRouteCutoverResult, IEdgeRepository, IMcpCredentialLifecycleRepository,
    IMcpCredentialRepository, IMcpRoutePolicyRepository, McpRoutePolicyWrite,
    McpRoutePolicyWriteSnapshot, MutateMcpRoutePolicyWrite, StageGatewayCertificateConvergence,
    StageGatewayRollout, StageGatewayRolloutRollback, StageGatewayRouteCutover,
    TransitionDomainClaim, MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY,
};
pub use domain::services::{
    DomainOwnershipVerificationError, DomainOwnershipVerificationRequest,
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, GatewayObservationCommand,
    GatewayObservationCommandOutcome, GatewayObservationDispatch, IDomainOwnershipVerifier,
    IGatewayCertificateAuthority, IGatewayCommandQueue, IGatewayObservationQueue,
    IMcpCredentialIssuer, IMcpRouteProjectionInputReader, IRouteTargetReader, IssuedMcpCredential,
    McpCredentialIssuanceError, McpCredentialIssueRequest, ResolvedMcpRouteProjectionInput,
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
    GatewayScopeState, McpCredential, McpCredentialDeliveryReceipt, McpRoutePolicy,
    McpRoutePolicyDocument, McpRoutePolicySpec, Route, RouteHostname, RoutePath, RoutePortName,
    RouteState, RouteTarget, UpstreamEndpoint, MCP_ROUTE_POLICY_MAX_ACL_BYTES,
};
pub use infrastructure::persistence::{InMemoryEdgeRepository, PostgresEdgeRepository};
pub use infrastructure::{
    CompileManagedGatewayCertificateConvergenceSnapshot, CompileManagedGatewayRetainedSnapshot,
    CompileManagedGatewayRolloutRollback, CompileManagedGatewayRouteRollout,
    CompileManagedGatewayRouteSnapshot, CompileMcpGatewaySnapshot, CompiledGatewayRouteRollout,
    CompiledMcpGatewaySnapshot, DnsDomainOwnershipVerifier, EdgeDeploymentRouteUpdater,
    EdgeGatewayAcknowledgementProjector, FleetGatewayCommandQueue, FleetGatewayObservationQueue,
    GatewayCertificateReconciler, GatewayCertificateReconciliationFailure,
    GatewayCertificateReconciliationReport, GatewayDomainClaimVersion,
    GatewayNodeDesiredStatePlanner, GatewayReplicaRecoveryReconciler,
    GatewayReplicaRecoveryReconciliationFailure, GatewayReplicaRecoveryReconciliationReport,
    GatewayRolloutReconciler, GatewayRolloutReconciliationFailure,
    GatewayRolloutReconciliationReport, GatewayRolloutRollbackCompiler,
    GatewayRolloutRollbackReconciler, GatewayRolloutRollbackReconciliationFailure,
    GatewayRolloutRollbackReconciliationReport, GatewayRouteRolloutCompiler,
    GatewayRouteRolloutPlanner, GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig,
    GatewaySnapshotMetadata, GatewaySnapshotRouteInput, IMcpGatewayNodeProjectionPlanner,
    IMcpGatewayProjectionSetPlanner, IMcpGatewaySnapshotRepository, LocalDomainOwnershipVerifier,
    LocalGatewayCertificateAuthority, McpCredentialAuthorityVersion, McpCredentialIssuer,
    McpCredentialProjectionVersion, McpGatewayDesiredStateReconciler,
    McpGatewayDesiredStateReconciliationFailure, McpGatewayDesiredStateReconciliationReport,
    McpGatewayIngressRoute, McpGatewayNodeProjectionPlanner, McpGatewayProjectionAssembler,
    McpGatewayProjectionCompiler, McpGatewayProjectionPlanner, McpGatewayProjectionSetPlanner,
    McpGatewayReconciliationScope, McpGatewaySnapshotDispatchTarget, McpGatewaySnapshotInputs,
    McpGatewaySnapshotReconciler, McpGatewaySnapshotReconciliationFailure,
    McpGatewaySnapshotReconciliationReport, McpGatewaySnapshotReconciliationState,
    McpGatewaySnapshotScopeStatus, McpGatewaySnapshotStageResult, McpGatewaySnapshotStatus,
    McpRouteProjectionInputReader, McpRouteProjectionPlanner, McpRouteProjectionVersion,
    McpRouteTargetCandidate, McpRouteTargetProjectionCompiler, PlanGatewayRouteRollout,
    PlanManagedGatewayRouteRollout, PlanMcpGatewayNodeProjection, PlanMcpGatewayProjectionSet,
    PlanMcpRouteProjection, PlannedMcpGatewayNodeProjection, PlannedMcpGatewayProjection,
    PlannedMcpGatewayProjectionSet, StageManagedRoutePublication, StageMcpGatewaySnapshot,
    VaultGatewayCertificateAuthority, WorkloadRouteTargetReader,
};
pub use presentation::EdgeModule;
