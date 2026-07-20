mod edge_repository;

pub use edge_repository::{
    CreateDomainClaimWrite, EdgeRoutePublicationResult, GatewayCertificateConvergenceResult,
    GatewayCertificateConvergenceTarget, GatewayCertificateRouteStatus, GatewayRouteCutoverResult,
    IEdgeRepository, StageGatewayCertificateConvergence, StageGatewayRouteCutover,
    StageRoutePublication, TransitionDomainClaim,
};
