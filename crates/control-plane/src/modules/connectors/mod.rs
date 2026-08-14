pub mod domain;
pub mod infrastructure;

pub use domain::{
    ConnectorExecutionError, ConnectorExecutionReceipt, ConnectorExecutionRequest,
    ConnectorHttpAuthentication, ConnectorHttpDefinition, ConnectorHttpDefinitionSpec,
    ConnectorHttpDestination, ConnectorHttpMethod, ConnectorHttpStatusPolicy,
    ConnectorSecretBinding, ConnectorSecretBindingPurpose, ConnectorSecretReference,
    IConnectorEgressAuthorizer, IConnectorExecutionPort, CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES,
    CONNECTOR_HTTP_DEFINITION_SCHEMA,
};
pub use infrastructure::{
    BoundedHttpConnectorExecutor, ResolvedConnectorAuthentication, ResolvedConnectorHttpRevision,
};
