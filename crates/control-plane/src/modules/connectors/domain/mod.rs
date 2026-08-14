mod execution;
mod http_definition;
mod http_policy;

pub(crate) use execution::{
    validate_connector_content_type, validate_connector_signature_metadata,
    MAXIMUM_CONNECTOR_BODY_BYTES,
};
pub use execution::{
    ConnectorExecutionError, ConnectorExecutionReceipt, ConnectorExecutionRequest,
    IConnectorEgressAuthorizer, IConnectorExecutionPort,
};
#[cfg(test)]
pub(crate) use http_definition::MINIMUM_SIGNING_SECRET_BYTES;
pub(crate) use http_definition::{
    maximum_connector_retry_after, validate_connector_http_limits,
    validate_connector_signing_secret_length, validate_resolved_connector_endpoint,
};
pub use http_definition::{
    ConnectorHttpAuthentication, ConnectorHttpDefinition, ConnectorHttpDefinitionSpec,
    ConnectorHttpDestination, ConnectorSecretBinding, ConnectorSecretBindingPurpose,
    ConnectorSecretReference, CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES,
    CONNECTOR_HTTP_DEFINITION_SCHEMA,
};
pub(crate) use http_policy::ConnectorStatusDisposition;
pub use http_policy::{ConnectorHttpMethod, ConnectorHttpStatusPolicy};
