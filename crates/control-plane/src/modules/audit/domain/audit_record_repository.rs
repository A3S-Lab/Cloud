use super::{
    AuditRecord, AuditRecordCursor, AuditRecordFilter, AuditRetentionReport, AuditRetentionState,
    AuditRetentionSweep,
};
use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError};
use async_trait::async_trait;

#[async_trait]
pub trait IAuditRecordRepository: Send + Sync {
    /// Returns one keyset page ordered by occurrence time and audit ID descending. The
    /// implementation must hold a shared retention-state lock across boundary validation and
    /// record selection so a page can never cross concurrently advanced retention authority.
    async fn list_page(
        &self,
        organization_id: OrganizationId,
        filter: &AuditRecordFilter,
        after: Option<AuditRecordCursor>,
        limit: usize,
    ) -> Result<Vec<AuditRecord>, RepositoryError>;

    async fn retention_state(
        &self,
        organization_id: OrganizationId,
    ) -> Result<AuditRetentionState, RepositoryError>;

    /// Advances per-tenant availability watermarks and performs bounded physical deletion in one
    /// transaction. A failed call must expose neither a new watermark nor partial deletion.
    async fn sweep_retention(
        &self,
        sweep: AuditRetentionSweep,
    ) -> Result<AuditRetentionReport, RepositoryError>;
}
