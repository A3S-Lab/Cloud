pub mod commands;
pub mod queries;

pub use commands::{
    CreateDomainClaim, CreateDomainClaimHandler, CreateDomainClaimResult, CreateGatewayScope,
    CreateGatewayScopeHandler, CreateGatewayScopeResult, PublishRoute, PublishRouteHandler,
    PublishRouteResult, RevokeDomainClaim, RevokeDomainClaimHandler, RevokeDomainClaimResult,
    SignGatewayCertificate, SignGatewayCertificateHandler, VerifyDomainClaim,
    VerifyDomainClaimHandler, VerifyDomainClaimResult,
};
pub use queries::{
    GetDomainClaim, GetDomainClaimHandler, GetRoute, GetRouteHandler, ListDomainClaims,
    ListDomainClaimsHandler, ListGatewayCertificates, ListGatewayCertificatesHandler,
    ListGatewayScopes, ListGatewayScopesHandler, ListRoutes, ListRoutesHandler,
};

#[cfg(test)]
mod tests;
