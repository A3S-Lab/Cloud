pub mod commands;
mod mcp_credential_lifecycle;
pub mod queries;

pub use mcp_credential_lifecycle::{
    McpCredentialLifecycleService, McpCredentialMutationResult, McpCredentialSecret,
};

pub use commands::{
    CreateDomainClaim, CreateDomainClaimHandler, CreateDomainClaimResult, CreateGatewayScope,
    CreateGatewayScopeHandler, CreateGatewayScopeResult, IssueMcpCredential,
    IssueMcpCredentialHandler, PublishRoute, PublishRouteHandler, PublishRouteResult,
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
