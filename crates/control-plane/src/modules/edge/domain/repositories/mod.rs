mod edge_repository;

pub use edge_repository::{
    CreateDomainClaimWrite, CreateGatewayScopeWrite, EdgeRoutePublicationResult,
    GatewayCertificateConvergenceResult, GatewayCertificateConvergenceTarget,
    GatewayCertificateRouteStatus, GatewayRouteCutoverResult, IEdgeRepository,
    StageGatewayCertificateConvergence, StageGatewayRouteCutover, StageRoutePublication,
    TransitionDomainClaim,
};
