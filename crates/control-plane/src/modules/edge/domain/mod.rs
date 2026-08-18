mod entities;
pub mod events;
pub mod repositories;
pub mod services;
mod value_objects;

pub use entities::{
    mcp_credential_delivery_context, DomainClaim, DomainClaimState, GatewayCertificate,
    GatewayCertificateConvergence, GatewayCertificateConvergenceReason,
    GatewayCertificateConvergenceState, GatewayCertificateMaterial, GatewayCertificateState,
    GatewayPublication, GatewayPublicationState, GatewayReplicaRecovery,
    GatewayReplicaRecoveryState, GatewayReplicaRollout, GatewayReplicaRolloutState, GatewayRollout,
    GatewayRolloutRollback, GatewayRolloutRollbackState, GatewayRolloutState, GatewayRouteCutover,
    GatewayRouteCutoverState, GatewayRouteVersion, GatewayScope, GatewayScopeState, McpCredential,
    McpCredentialDeliveryReceipt, McpRoutePolicy, McpRoutePolicyDocument, McpRoutePolicySpec,
    Route, RouteState, MCP_ROUTE_POLICY_MAX_ACL_BYTES,
};
pub(crate) use value_objects::GatewaySnapshotRuntimeSettings;
pub use value_objects::{
    DomainNamePattern, GatewayRolloutPolicy, RouteHostname, RoutePath, RoutePortName, RouteTarget,
    UpstreamEndpoint, MAX_GATEWAY_SCOPE_MEMBERS,
};

#[cfg(test)]
mod tests;
