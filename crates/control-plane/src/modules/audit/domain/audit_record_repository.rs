use super::{AuditRecord, AuditRecordCursor, AuditRecordFilter};
use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError};
use async_trait::async_trait;

#[async_trait]
pub trait IAuditRecordRepository: Send + Sync {
    /// Returns one keyset page ordered by occurrence time and audit ID descending.
    async fn list_page(
        &self,
        organization_id: OrganizationId,
        filter: &AuditRecordFilter,
        after: Option<AuditRecordCursor>,
        limit: usize,
    ) -> Result<Vec<AuditRecord>, RepositoryError>;
}
