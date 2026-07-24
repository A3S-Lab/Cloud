mod edge_repository;

pub use edge_repository::{
    CreateDomainClaimWrite, CreateGatewayScopeWrite, EdgeRoutePublicationResult,
    GatewayCertificateConvergenceResult, GatewayCertificateConvergenceTarget,
    GatewayCertificateRouteStatus, GatewayRolloutResult, GatewayRouteCutoverResult,
    IEdgeRepository, StageGatewayCertificateConvergence, StageGatewayRollout,
    StageGatewayRouteCutover, StageRoutePublication, TransitionDomainClaim,
};
