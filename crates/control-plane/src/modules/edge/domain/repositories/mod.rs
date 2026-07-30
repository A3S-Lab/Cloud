mod edge_repository;
mod mcp_credential_repository;
mod mcp_route_policy_repository;

pub use edge_repository::{
    CreateDomainClaimWrite, CreateGatewayScopeWrite, EdgeRoutePublicationResult,
    GatewayCertificateConvergenceResult, GatewayCertificateConvergenceTarget,
    GatewayCertificateRouteStatus, GatewayReplicaRecoveryTarget, GatewayRolloutDispatchTarget,
    GatewayRolloutResult, GatewayRolloutRollbackResult, GatewayRolloutRollbackTarget,
    GatewayRouteCutoverResult, IEdgeRepository, StageGatewayCertificateConvergence,
    StageGatewayRollout, StageGatewayRolloutRollback, StageGatewayRouteCutover,
    StageRoutePublication, TransitionDomainClaim,
};
pub(crate) use mcp_credential_repository::validate_mcp_credential_resolution;
pub use mcp_credential_repository::IMcpCredentialRepository;
pub use mcp_route_policy_repository::{
    IMcpRoutePolicyRepository, MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY,
};
