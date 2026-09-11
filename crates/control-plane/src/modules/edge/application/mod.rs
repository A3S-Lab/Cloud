pub mod commands;
mod environment_access;
mod mcp_credential_delivery;
mod mcp_credential_delivery_receipt_sweeper;
mod mcp_credential_encryption;
mod mcp_route_policy_service;
mod mcp_service_profile_access;
mod mcp_workload_revision_projection_access;
mod node_access;
pub mod queries;
mod resource_access;
mod runtime_observation_access;

pub use environment_access::{EdgeEnvironmentScope, IEdgeEnvironmentAccess};
pub use mcp_credential_delivery::{
    McpCredentialDeliveryResult, McpCredentialMutationResult,
    MCP_CREDENTIAL_DELIVERY_RECEIPT_TTL_SECONDS,
};
pub use mcp_credential_delivery_receipt_sweeper::McpCredentialDeliveryReceiptSweeper;
pub use mcp_credential_encryption::IEdgeMcpCredentialEncryption;
pub use mcp_route_policy_service::McpRoutePolicyApplicationService;
pub use mcp_service_profile_access::{EdgeMcpServiceProfileScope, IEdgeMcpServiceProfileAccess};
pub use mcp_workload_revision_projection_access::{
    EdgeMcpWorkloadRevisionProjectionScope, IEdgeMcpWorkloadRevisionProjectionAccess,
};
pub use node_access::{EdgeNodeScope, IEdgeNodeAccess};
pub use runtime_observation_access::{EdgeRuntimeObservationProjection, IEdgeRuntimeObservationAccess};

pub use commands::{
    CreateDomainClaim, CreateDomainClaimHandler, CreateDomainClaimResult, CreateGatewayScope,
    CreateGatewayScopeHandler, CreateGatewayScopeResult, CreateMcpCredential,
    CreateMcpCredentialHandler, CreateMcpRoutePolicy, CreateMcpRoutePolicyHandler, PublishRoute,
    PublishRouteHandler, PublishRouteResult, ReviseMcpRoutePolicy, ReviseMcpRoutePolicyHandler,
    RevokeDomainClaim, RevokeDomainClaimHandler, RevokeDomainClaimResult, RevokeMcpCredential,
    RevokeMcpCredentialHandler, RotateMcpCredential, RotateMcpCredentialHandler,
    SignGatewayCertificate, SignGatewayCertificateHandler, VerifyDomainClaim,
    VerifyDomainClaimHandler, VerifyDomainClaimResult,
};
pub use queries::{
    GetDomainClaim, GetDomainClaimHandler, GetMcpCredential, GetMcpCredentialHandler,
    GetMcpRoutePolicy, GetMcpRoutePolicyHandler, GetRoute, GetRouteHandler, ListDomainClaims,
    ListDomainClaimsHandler, ListGatewayCertificates, ListGatewayCertificatesHandler,
    ListGatewayScopes, ListGatewayScopesHandler, ListMcpCredentials, ListMcpCredentialsHandler,
    ListMcpRoutePolicies, ListMcpRoutePoliciesHandler, ListRoutes, ListRoutesHandler,
};

#[cfg(test)]
mod tests;
