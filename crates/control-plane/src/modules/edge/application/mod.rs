pub mod commands;
pub mod queries;

pub use commands::{
    CreateDomainClaim, CreateDomainClaimHandler, CreateDomainClaimResult, PublishRoute,
    PublishRouteHandler, PublishRouteResult, VerifyDomainClaim, VerifyDomainClaimHandler,
    VerifyDomainClaimResult,
};
pub use queries::{
    GetDomainClaim, GetDomainClaimHandler, GetRoute, GetRouteHandler, ListDomainClaims,
    ListDomainClaimsHandler, ListGatewayCertificates, ListGatewayCertificatesHandler, ListRoutes,
    ListRoutesHandler,
};

#[cfg(test)]
mod tests;
