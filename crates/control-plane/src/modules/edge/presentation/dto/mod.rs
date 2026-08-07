mod request;
mod response;

pub use request::{
    CreateDomainClaimRequest, CreateGatewayScopeRequest, CreateMcpCredentialRequest,
    PublishRouteRequest, RevokeDomainClaimRequest, RevokeMcpCredentialRequest,
    RotateMcpCredentialRequest, VerifyDomainClaimRequest,
};
pub use response::{
    DomainClaimMutationResponse, DomainClaimResponse, GatewayCertificateResponse,
    GatewayScopeMutationResponse, GatewayScopeResponse, McpCredentialDeliveryResponse,
    McpCredentialMutationResponse, McpCredentialResponse, McpRoutePolicyMutationResponse,
    McpRoutePolicyResponse, RoutePublicationResponse, RouteResponse,
};
