mod attempt;
mod attempt_repository;
mod events;
mod evidence;
mod evidence_repository;
mod execution;
mod http_definition;
mod http_policy;
mod profile;
mod repository;
mod response_object;

pub use events::ConnectorRevisionPublished;
pub use evidence::{
    ConnectorExecutionEvidence, ConnectorExecutionEvidenceCursor, ConnectorExecutionEvidencePage,
    ConnectorExecutionOutcome, MAXIMUM_CONNECTOR_EXECUTION_EVIDENCE_PAGE_SIZE,
};
pub use evidence_repository::IConnectorExecutionEvidenceRepository;

pub use attempt::{
    ConnectorExecutionAttempt, ConnectorExecutionAttemptBinding, ConnectorExecutionAttemptCursor,
    ConnectorExecutionAttemptPage, ConnectorExecutionAttemptRecord, ConnectorExecutionAttemptState,
    ConnectorExecutionFence, ConnectorExecutionRecoveryState,
    MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE, MAXIMUM_CONNECTOR_EXECUTION_OUTCOME_SECONDS,
    MAXIMUM_CONNECTOR_EXECUTION_RESERVATION_SECONDS,
};
pub(crate) use attempt_repository::reservation_record;
pub use attempt_repository::{
    BeginConnectorExecutionDispatch, ConnectorExecutionReservation,
    IConnectorExecutionAttemptRepository, ReserveConnectorExecutionAttempt,
    SettleConnectorExecutionAttempt,
};
pub(crate) use execution::{
    validate_connector_content_type, validate_connector_signature_metadata,
    MAXIMUM_AUTHORIZED_CONNECTOR_ADDRESSES, MAXIMUM_CONNECTOR_BODY_BYTES,
};
pub use execution::{
    AuthorizedConnectorDestination, ConnectorExecutionError, ConnectorExecutionReceipt,
    ConnectorExecutionRequest, IConnectorEgressAuthorizer, IConnectorExecutionPort,
    IConnectorExecutionPreparationPort, IPreparedConnectorExecution,
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
pub use profile::{ConnectorDefinition, ConnectorProfile, ConnectorRevision};
pub(crate) use repository::ConnectorWriteReference;
pub use repository::{
    ConnectorRecord, CreateConnectorProfileWrite, IConnectorProfileRepository,
    ReviseConnectorProfileWrite,
};
pub use response_object::{
    ConnectorResponseObjectError, ConnectorResponseObjectReference, ConnectorResponseObjectWrite,
    IConnectorResponseObjectStore, CONNECTOR_RESPONSE_OBJECT_SCHEMA,
};
