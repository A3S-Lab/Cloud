mod domain_ownership_verifier;
mod gateway_certificate_authority;
mod gateway_command_queue;
mod gateway_observation_queue;
mod mcp_credential_issuer;
mod mcp_route_projection_input_reader;
mod route_target_reader;

pub use domain_ownership_verifier::{
    DomainOwnershipVerificationError, DomainOwnershipVerificationRequest, IDomainOwnershipVerifier,
};
pub use gateway_certificate_authority::{
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, IGatewayCertificateAuthority,
};
pub use gateway_command_queue::{GatewayCommandDispatch, IGatewayCommandQueue};
pub use gateway_observation_queue::{
    GatewayObservationCommand, GatewayObservationCommandOutcome, GatewayObservationDispatch,
    IGatewayObservationQueue,
};
pub use mcp_credential_issuer::{
    validate_lifetime as validate_mcp_credential_lifetime, IMcpCredentialIssuer,
    IssuedMcpCredential, McpCredentialIssuanceError, McpCredentialIssueRequest,
    MAX_MCP_CREDENTIAL_LIFETIME_DAYS,
};
pub use mcp_route_projection_input_reader::{
    IMcpRouteProjectionInputReader, ResolvedMcpRouteProjectionInput,
};
pub use route_target_reader::{IRouteTargetReader, ResolvedRouteTarget, ResolvedRouteTargetSet};
