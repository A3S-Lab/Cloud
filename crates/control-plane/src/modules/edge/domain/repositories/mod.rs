mod edge_repository;

pub use edge_repository::{
    CreateDomainClaimWrite, CreateGatewayScopeWrite, EdgeRoutePublicationResult,
    GatewayCertificateConvergenceResult, GatewayCertificateConvergenceTarget,
    GatewayCertificateRouteStatus, GatewayRolloutDispatchTarget, GatewayRolloutResult,
    GatewayRouteCutoverResult, IEdgeRepository, StageGatewayCertificateConvergence,
    StageGatewayRollout, StageGatewayRouteCutover, StageRoutePublication, TransitionDomainClaim,
};
