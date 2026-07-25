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
    GatewayRouteCutoverResult, IEdgeRepository, StageGatewayCertificateConvergence,
    StageGatewayRollout, StageGatewayRolloutRollback, StageGatewayRouteCutover,
    TransitionDomainClaim,
};
pub use domain::services::{
    DomainOwnershipVerificationError, DomainOwnershipVerificationRequest,
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, GatewayObservationCommand,
    GatewayObservationCommandOutcome, GatewayObservationDispatch, IDomainOwnershipVerifier,
    IGatewayCertificateAuthority, IGatewayCommandQueue, IGatewayObservationQueue,
    IRouteTargetReader, ResolvedRouteTarget, ResolvedRouteTargetSet,
};
pub use domain::{
    DomainClaim, DomainClaimState, DomainNamePattern, GatewayCertificate,
    GatewayCertificateConvergence, GatewayCertificateConvergenceReason,
    GatewayCertificateConvergenceState, GatewayCertificateMaterial, GatewayCertificateState,
    GatewayPublication, GatewayPublicationState, GatewayReplicaRecovery,
    GatewayReplicaRecoveryState, GatewayReplicaRollout, GatewayReplicaRolloutState, GatewayRollout,
    GatewayRolloutPolicy, GatewayRolloutRollback, GatewayRolloutRollbackState, GatewayRolloutState,
    GatewayRouteCutover, GatewayRouteCutoverState, GatewayRouteVersion, GatewayScope,
    GatewayScopeState, Route, RouteHostname, RoutePath, RoutePortName, RouteState, RouteTarget,
    UpstreamEndpoint,
};
pub use infrastructure::persistence::{InMemoryEdgeRepository, PostgresEdgeRepository};
pub use infrastructure::{
    DnsDomainOwnershipVerifier, EdgeDeploymentRouteUpdater, EdgeGatewayAcknowledgementProjector,
    FleetGatewayCommandQueue, FleetGatewayObservationQueue, GatewayCertificateReconciler,
    GatewayCertificateReconciliationFailure, GatewayCertificateReconciliationReport,
    GatewayReplicaRecoveryReconciler, GatewayReplicaRecoveryReconciliationFailure,
    GatewayReplicaRecoveryReconciliationReport, GatewayRolloutReconciler,
    GatewayRolloutReconciliationFailure, GatewayRolloutReconciliationReport,
    GatewayRolloutRollbackCompiler, GatewayRolloutRollbackReconciler,
    GatewayRolloutRollbackReconciliationFailure, GatewayRolloutRollbackReconciliationReport,
    GatewayRouteRolloutCompiler, GatewayRouteRolloutPlanner, GatewaySnapshotCompiler,
    GatewaySnapshotCompilerConfig, LocalDomainOwnershipVerifier, LocalGatewayCertificateAuthority,
    PlanGatewayRouteRollout, VaultGatewayCertificateAuthority, WorkloadRouteTargetReader,
};
pub use presentation::EdgeModule;
