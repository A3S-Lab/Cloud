use crate::modules::operations::domain::entities::{
    OperationProjection, OperationRecord, OperationRequest,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OperationId, OrganizationId, RepositoryError,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationListCursor {
    pub requested_at: DateTime<Utc>,
    pub operation_id: OperationId,
}

impl OperationListCursor {
    pub fn after(record: &OperationRecord) -> Self {
        Self {
            requested_at: record.request.requested_at,
            operation_id: record.request.id,
        }
    }
}

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
    ) -> Result<Vec<OperationRecord>, RepositoryError> {
        self.list_page(organization_id, None, limit).await
    }

    /// Returns one keyset page ordered by requested time descending and operation ID ascending.
    async fn list_page(
        &self,
        organization_id: OrganizationId,
        after: Option<OperationListCursor>,
        limit: usize,
    ) -> Result<Vec<OperationRecord>, RepositoryError>;
}
