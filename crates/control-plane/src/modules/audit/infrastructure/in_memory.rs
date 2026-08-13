use crate::modules::audit::domain::{
    AuditRecord, AuditRecordCursor, AuditRecordFilter, IAuditRecordRepository,
};
use crate::modules::shared_kernel::domain::{canonical_timestamp, OrganizationId, RepositoryError};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryAuditRecordRepository {
    records: RwLock<Vec<AuditRecord>>,
    query_count: AtomicUsize,
}

impl InMemoryAuditRecordRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(&self, mut record: AuditRecord) -> Result<(), RepositoryError> {
        record.occurred_at = canonical_timestamp(record.occurred_at);
        record.validate().map_err(RepositoryError::Storage)?;
        let mut records = self.records.write().await;
        if records.iter().any(|existing| existing.id == record.id) {
            return Err(RepositoryError::Conflict(
                "audit record ID is already in use".into(),
            ));
        }
        records.push(record);
        Ok(())
    }

    pub fn query_count(&self) -> usize {
        self.query_count.load(AtomicOrdering::Relaxed)
    }
}

#[async_trait]
impl IAuditRecordRepository for InMemoryAuditRecordRepository {
    async fn list_page(
        &self,
        organization_id: OrganizationId,
        filter: &AuditRecordFilter,
        after: Option<AuditRecordCursor>,
        limit: usize,
    ) -> Result<Vec<AuditRecord>, RepositoryError> {
        self.query_count.fetch_add(1, AtomicOrdering::Relaxed);
        let mut records = self
            .records
            .read()
            .await
            .iter()
            .filter(|record| record.organization_id == organization_id)
            .filter(|record| filter.matches(record))
            .filter(|record| {
                after.is_none_or(|cursor| {
                    record.occurred_at < cursor.occurred_at
                        || (record.occurred_at == cursor.occurred_at && record.id < cursor.audit_id)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            right
                .occurred_at
                .cmp(&left.occurred_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        records.truncate(limit.max(1));
        Ok(records)
    }
}
