pub mod commands;
mod mcp_credential_delivery;
pub mod queries;

pub(crate) use mcp_credential_delivery::{encrypt_delivery_receipt, recover_delivery};
pub use mcp_credential_delivery::{
    McpCredentialDeliveryResult, McpCredentialMutationResult,
    MCP_CREDENTIAL_DELIVERY_RECEIPT_TTL_SECONDS,
};

pub use commands::{
    CreateDomainClaim, CreateDomainClaimHandler, CreateDomainClaimResult, CreateGatewayScope,
    CreateGatewayScopeHandler, CreateGatewayScopeResult, CreateMcpCredential,
    CreateMcpCredentialHandler, PublishRoute, PublishRouteHandler, PublishRouteResult,
    RevokeDomainClaim, RevokeDomainClaimHandler, RevokeDomainClaimResult, RevokeMcpCredential,
    RevokeMcpCredentialHandler, RotateMcpCredential, RotateMcpCredentialHandler,
    SignGatewayCertificate, SignGatewayCertificateHandler, VerifyDomainClaim,
    VerifyDomainClaimHandler, VerifyDomainClaimResult,
};
pub use queries::{
    GetDomainClaim, GetDomainClaimHandler, GetMcpCredential, GetMcpCredentialHandler, GetRoute,
    GetRouteHandler, ListDomainClaims, ListDomainClaimsHandler, ListGatewayCertificates,
    ListGatewayCertificatesHandler, ListGatewayScopes, ListGatewayScopesHandler,
    ListMcpCredentials, ListMcpCredentialsHandler, ListRoutes, ListRoutesHandler,
};

#[cfg(test)]
mod tests;
