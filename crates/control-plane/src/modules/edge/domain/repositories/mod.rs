mod edge_repository;
mod mcp_credential_lifecycle_repository;
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
pub(crate) use mcp_credential_lifecycle_repository::McpCredentialLifecycleReference;
pub use mcp_credential_lifecycle_repository::{
    IMcpCredentialAuthorityRepository, IMcpCredentialLifecycleRepository,
    McpCredentialLifecycleResult, StoreMcpCredentialLifecycle,
    MAX_MCP_CREDENTIAL_DELIVERY_PURGE_BATCH,
};
pub use mcp_credential_repository::IMcpCredentialRepository;
pub(crate) use mcp_credential_repository::{
    validate_mcp_credential_resolution, MCP_CREDENTIAL_IDENTITY_CONFLICT,
};
pub use mcp_route_policy_repository::{
    IMcpRoutePolicyRepository, MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY,
};
