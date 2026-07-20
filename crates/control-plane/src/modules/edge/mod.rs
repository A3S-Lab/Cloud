pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    CreateDomainClaim, CreateDomainClaimHandler, CreateDomainClaimResult, GetDomainClaim,
    GetDomainClaimHandler, GetRoute, GetRouteHandler, ListDomainClaims, ListDomainClaimsHandler,
    ListGatewayCertificates, ListGatewayCertificatesHandler, ListRoutes, ListRoutesHandler,
    PublishRoute, PublishRouteHandler, PublishRouteResult, SignGatewayCertificate,
    SignGatewayCertificateHandler, VerifyDomainClaim, VerifyDomainClaimHandler,
    VerifyDomainClaimResult,
};
pub use domain::repositories::{
    CreateDomainClaimWrite, EdgeRoutePublicationResult, GatewayRouteCutoverResult, IEdgeRepository,
    StageGatewayRouteCutover, TransitionDomainClaim,
};
pub use domain::services::{
    DomainOwnershipVerificationError, DomainOwnershipVerificationRequest,
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, IDomainOwnershipVerifier,
    IGatewayCertificateAuthority, IGatewayCommandQueue, IRouteTargetReader, RouteTarget,
};
pub use domain::{
    DomainClaim, DomainClaimState, DomainNamePattern, GatewayCertificate,
    GatewayCertificateMaterial, GatewayCertificateState, GatewayPublication,
    GatewayPublicationState, GatewayRouteCutover, GatewayRouteCutoverState, GatewayScopeState,
    Route, RouteHostname, RoutePath, RoutePortName, RouteState, UpstreamEndpoint,
};
pub use infrastructure::persistence::{InMemoryEdgeRepository, PostgresEdgeRepository};
pub use infrastructure::{
    EdgeDeploymentRouteUpdater, EdgeGatewayAcknowledgementProjector, FleetGatewayCommandQueue,
    GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, LocalDomainOwnershipVerifier,
    LocalGatewayCertificateAuthority, UnavailableDomainOwnershipVerifier,
    UnavailableGatewayCertificateAuthority, WorkloadRouteTargetReader,
};
pub use presentation::EdgeModule;
