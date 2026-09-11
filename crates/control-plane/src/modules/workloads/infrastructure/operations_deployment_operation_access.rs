use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::shared_kernel::domain::{OperationId, RepositoryError};
use crate::modules::workloads::application::{
    IWorkloadDeploymentOperationAccess, WorkloadDeploymentOperationProjection,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Read-only anti-corruption adapter for Operations deployment enrichment.
#[derive(Clone)]
pub struct OperationsWorkloadDeploymentOperationAccessAdapter {
    operations: Arc<dyn IOperationRepository>,
}

impl OperationsWorkloadDeploymentOperationAccessAdapter {
    pub fn new(operations: Arc<dyn IOperationRepository>) -> Self {
        Self { operations }
    }
}

#[async_trait]
impl IWorkloadDeploymentOperationAccess for OperationsWorkloadDeploymentOperationAccessAdapter {
    async fn find_projection(
        &self,
        operation_id: OperationId,
    ) -> Result<Option<WorkloadDeploymentOperationProjection>, RepositoryError> {
        Ok(self
            .operations
            .find_projection(operation_id)
            .await?
            .map(|projection| WorkloadDeploymentOperationProjection {
                operation_id: projection.operation_id,
                status: projection.status.as_str().into(),
                last_sequence: projection.last_sequence,
                error: projection.error,
                updated_at: projection.updated_at,
            }))
    }
}
