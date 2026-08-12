pub mod commands;
mod mcp_credential_delivery;
mod mcp_credential_delivery_receipt_sweeper;
mod mcp_route_policy_service;
pub mod queries;
mod resource_access;

pub(crate) use mcp_credential_delivery::{encrypt_delivery_receipt, recover_delivery};
pub use mcp_credential_delivery::{
    McpCredentialDeliveryResult, McpCredentialMutationResult,
    MCP_CREDENTIAL_DELIVERY_RECEIPT_TTL_SECONDS,
};
pub use mcp_credential_delivery_receipt_sweeper::McpCredentialDeliveryReceiptSweeper;
pub use mcp_route_policy_service::McpRoutePolicyApplicationService;

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
