use super::{BoundedHttpConnectorExecutor, ConnectorHttpRevisionMaterializer};
use crate::modules::connectors::domain::{
    ConnectorExecutionError, ConnectorExecutionRequest, ConnectorRevision,
    IConnectorEgressAuthorizer, IConnectorExecutionPreparationPort, IPreparedConnectorExecution,
};
use crate::modules::shared_kernel::application::ApplicationError;
use async_trait::async_trait;
use std::fmt;
use std::sync::Arc;

/// The production HTTP preparation adapter. It reuses the sole Secret
/// materializer, egress authorizer, and bounded HTTP executor; it owns no
/// transport, cache, credential state, retry loop, or scheduler of its own.
#[derive(Clone)]
pub struct ConnectorHttpExecutionPreparationPort {
    materializer: ConnectorHttpRevisionMaterializer,
    egress: Arc<dyn IConnectorEgressAuthorizer>,
}

impl ConnectorHttpExecutionPreparationPort {
    pub fn new(
        materializer: ConnectorHttpRevisionMaterializer,
        egress: Arc<dyn IConnectorEgressAuthorizer>,
    ) -> Self {
        Self {
            materializer,
            egress,
        }
    }
}

impl fmt::Debug for ConnectorHttpExecutionPreparationPort {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectorHttpExecutionPreparationPort")
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl IConnectorExecutionPreparationPort for ConnectorHttpExecutionPreparationPort {
    async fn prepare(
        &self,
        revision: &ConnectorRevision,
        request: &ConnectorExecutionRequest,
    ) -> Result<Box<dyn IPreparedConnectorExecution>, ConnectorExecutionError> {
        revision
            .validate()
            .map_err(|_| ConnectorExecutionError::Rejected)?;
        request
            .validate()
            .map_err(|_| ConnectorExecutionError::Rejected)?;
        if revision.id != request.connector_revision_id() {
            return Err(ConnectorExecutionError::Rejected);
        }
        let resolved = self
            .materializer
            .materialize(revision)
            .await
            .map_err(classify_materialization_error)?;
        let prepared = BoundedHttpConnectorExecutor::new(resolved, self.egress.clone())
            .prepare(request)
            .await?;
        Ok(Box::new(prepared))
    }
}

fn classify_materialization_error(error: ApplicationError) -> ConnectorExecutionError {
    match error {
        ApplicationError::Unavailable(_) | ApplicationError::Internal(_) => {
            ConnectorExecutionError::Retryable { retry_after: None }
        }
        ApplicationError::Invalid(_)
        | ApplicationError::NotFound(_)
        | ApplicationError::Conflict(_)
        | ApplicationError::Forbidden(_) => ConnectorExecutionError::Rejected,
    }
}
