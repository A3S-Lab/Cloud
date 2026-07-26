mod request;
mod response;

pub use request::{
    CreateDomainClaimRequest, CreateGatewayScopeRequest, PublishRouteRequest,
    RevokeDomainClaimRequest, VerifyDomainClaimRequest,
};
pub use response::{
    DomainClaimMutationResponse, DomainClaimResponse, GatewayCertificateResponse,
    GatewayScopeMutationResponse, GatewayScopeResponse, RoutePublicationResponse, RouteResponse,
};
