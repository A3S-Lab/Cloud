use crate::modules::operations::domain::entities::{
    OperationProjection, OperationRecord, OperationRequest,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OperationId, OrganizationId, RepositoryError,
};
use async_trait::async_trait;

#[async_trait]
pub trait IOperationRepository: Send + Sync {
    async fn enqueue(
        &self,
        request: OperationRequest,
    ) -> Result<IdempotentWrite<OperationRequest>, RepositoryError>;

    async fn pending_starts(&self, limit: usize) -> Result<Vec<OperationRequest>, RepositoryError>;

    async fn find_request(
        &self,
        operation_id: OperationId,
    ) -> Result<Option<OperationRequest>, RepositoryError>;

    async fn upsert_projection(
        &self,
        projection: OperationProjection,
    ) -> Result<(), RepositoryError>;

    async fn find_projection(
        &self,
        operation_id: OperationId,
    ) -> Result<Option<OperationProjection>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        limit: usize,
    ) -> Result<Vec<OperationRecord>, RepositoryError>;
}
