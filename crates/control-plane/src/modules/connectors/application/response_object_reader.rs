use super::{
    execution_service::ConnectorExecutionApplicationService, resource_access::environment,
};
use crate::modules::connectors::domain::{
    ConnectorResponseObjectError, ConnectorResponseObjectReference,
    IConnectorExecutionAttemptRepository, IConnectorResponseObjectStore,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{RepositoryError, Sha256Digest};
use async_trait::async_trait;
use std::fmt;

/// Requests one exact response object through its owning Connector authority.
///
/// This is an internal application-port contract. It is deliberately not a
/// public download DTO and carries no caller-selected object-storage key.
#[derive(Clone)]
pub struct ReadConnectorResponseObject {
    pub reference: ConnectorResponseObjectReference,
    pub resource_access: ResourceAccessEvaluator,
}

impl fmt::Debug for ReadConnectorResponseObject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReadConnectorResponseObject")
            .field("organization_id", &self.reference.organization_id)
            .field("project_id", &self.reference.project_id)
            .field("environment_id", &self.reference.environment_id)
            .field("connector_profile_id", &self.reference.connector_profile_id)
            .field(
                "connector_revision_id",
                &self.reference.connector_revision_id,
            )
            .field("connector_attempt_id", &self.reference.connector_attempt_id)
            .field("size_bytes", &self.reference.size_bytes)
            .finish_non_exhaustive()
    }
}

/// Exact transient bytes returned only after access, evidence, and integrity
/// validation. Debug output intentionally excludes the object path, digest,
/// and body.
#[derive(PartialEq, Eq)]
pub struct ConnectorResponseObjectContent {
    reference: ConnectorResponseObjectReference,
    body: Vec<u8>,
}

impl ConnectorResponseObjectContent {
    fn new(reference: ConnectorResponseObjectReference, body: Vec<u8>) -> ApplicationResult<Self> {
        reference.validate().map_err(ApplicationError::Internal)?;
        if body.len() as u64 != reference.size_bytes
            || Sha256Digest::from_bytes(&body) != reference.digest
        {
            return Err(ApplicationError::Internal(
                "Connector response object changed after its integrity check".into(),
            ));
        }
        Ok(Self { reference, body })
    }

    pub const fn reference(&self) -> &ConnectorResponseObjectReference {
        &self.reference
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn into_body(self) -> Vec<u8> {
        self.body
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        reference: ConnectorResponseObjectReference,
        body: Vec<u8>,
    ) -> ApplicationResult<Self> {
        Self::new(reference, body)
    }
}

impl fmt::Debug for ConnectorResponseObjectContent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectorResponseObjectContent")
            .field("organization_id", &self.reference.organization_id)
            .field("project_id", &self.reference.project_id)
            .field("environment_id", &self.reference.environment_id)
            .field("connector_attempt_id", &self.reference.connector_attempt_id)
            .field("size_bytes", &self.reference.size_bytes)
            .finish()
    }
}

#[async_trait]
pub trait IConnectorResponseObjectPort: Send + Sync {
    /// Resolves exact response bytes only when the immutable object is backed
    /// by accepted terminal C6 evidence in the same authorized environment.
    async fn read_response_object(
        &self,
        request: &ReadConnectorResponseObject,
    ) -> ApplicationResult<ConnectorResponseObjectContent>;
}

#[async_trait]
impl IConnectorResponseObjectPort for ConnectorExecutionApplicationService {
    async fn read_response_object(
        &self,
        request: &ReadConnectorResponseObject,
    ) -> ApplicationResult<ConnectorResponseObjectContent> {
        read_response_object(
            self.attempts.as_ref(),
            self.response_objects.as_deref(),
            request,
        )
        .await
    }
}

pub(super) async fn read_response_object(
    attempts: &dyn IConnectorExecutionAttemptRepository,
    objects: Option<&dyn IConnectorResponseObjectStore>,
    request: &ReadConnectorResponseObject,
) -> ApplicationResult<ConnectorResponseObjectContent> {
    let reference = &request.reference;
    environment(
        reference.project_id,
        reference.environment_id,
        &request.resource_access,
    )?;
    reference.validate().map_err(ApplicationError::Invalid)?;

    let record = match attempts
        .find(
            reference.organization_id,
            reference.project_id,
            reference.environment_id,
            reference.connector_profile_id,
            reference.connector_revision_id,
            reference.connector_attempt_id,
        )
        .await
    {
        Ok(Some(record)) => record,
        Ok(None) | Err(RepositoryError::NotFound) => return Err(response_object_not_found()),
        Err(error) => return Err(error.into()),
    };
    let evidence = record
        .evidence
        .as_ref()
        .ok_or_else(response_object_not_found)?;
    reference.validate_evidence(evidence).map_err(|_| {
        ApplicationError::Conflict(
            "Connector response object changed its terminal evidence authority".into(),
        )
    })?;

    let objects = objects.ok_or_else(|| {
        ApplicationError::Unavailable("Connector response-object storage is not configured".into())
    })?;
    let body = objects
        .get(reference)
        .await
        .map_err(map_response_object_error)?;
    ConnectorResponseObjectContent::new(reference.clone(), body)
}

fn response_object_not_found() -> ApplicationError {
    ApplicationError::NotFound("Connector response object not found".into())
}

pub(super) fn map_response_object_error(error: ConnectorResponseObjectError) -> ApplicationError {
    match error {
        ConnectorResponseObjectError::Unavailable(message) => {
            ApplicationError::Unavailable(message)
        }
        ConnectorResponseObjectError::Invalid(message)
        | ConnectorResponseObjectError::Conflict(message)
        | ConnectorResponseObjectError::Integrity(message) => ApplicationError::Internal(message),
        ConnectorResponseObjectError::NotFound => ApplicationError::Internal(
            "accepted Connector evidence references a missing response object".into(),
        ),
    }
}
