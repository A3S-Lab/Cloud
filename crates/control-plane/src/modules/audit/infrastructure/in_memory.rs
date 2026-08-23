use crate::modules::audit::domain::{
    validate_retained_query_window, AuditExportSnapshot, AuditRecord, AuditRecordCursor,
    AuditRecordFilter, AuditRetentionReport, AuditRetentionState, AuditRetentionSweep,
    IAuditRecordRepository,
};
use crate::modules::shared_kernel::domain::{canonical_timestamp, OrganizationId, RepositoryError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Default)]
struct AuditStore {
    records: Vec<AuditRecord>,
    retention: BTreeMap<OrganizationId, AuditRetentionState>,
}

#[derive(Default)]
pub struct InMemoryAuditRecordRepository {
    store: RwLock<AuditStore>,
    query_count: AtomicUsize,
}

impl InMemoryAuditRecordRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register_organization(&self, organization_id: OrganizationId) {
        self.store
            .write()
            .await
            .retention
            .entry(organization_id)
            .or_insert_with(|| initial_state(organization_id));
    }

    pub async fn register(&self, mut record: AuditRecord) -> Result<(), RepositoryError> {
        record.occurred_at = canonical_timestamp(record.occurred_at);
        record.validate().map_err(RepositoryError::Storage)?;
        let mut store = self.store.write().await;
        let state = store
            .retention
            .entry(record.organization_id)
            .or_insert_with(|| initial_state(record.organization_id));
        if state
            .records_available_from
            .is_some_and(|boundary| record.occurred_at < boundary)
        {
            return Err(RepositoryError::Conflict(
                "audit record is older than the retained availability boundary".into(),
            ));
        }
        if store
            .records
            .iter()
            .any(|existing| existing.id == record.id)
        {
            return Err(RepositoryError::Conflict(
                "audit record ID is already in use".into(),
            ));
        }
        store.records.push(record);
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
        let store = self.store.read().await;
        let boundary = store
            .retention
            .get(&organization_id)
            .and_then(|state| state.records_available_from);
        validate_retained_query_window(boundary, filter, after)
            .map_err(RepositoryError::Conflict)?;
        let mut records = store
            .records
            .iter()
            .filter(|record| record.organization_id == organization_id)
            .filter(|record| boundary.is_none_or(|value| record.occurred_at >= value))
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

    async fn retention_state(
        &self,
        organization_id: OrganizationId,
    ) -> Result<AuditRetentionState, RepositoryError> {
        self.store
            .read()
            .await
            .retention
            .get(&organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn capture_export_snapshot(
        &self,
        organization_id: OrganizationId,
        filter: &AuditRecordFilter,
        maximum_records: usize,
    ) -> Result<AuditExportSnapshot, RepositoryError> {
        if maximum_records == 0 {
            return Err(RepositoryError::Storage(
                "audit export snapshot bound must be positive".into(),
            ));
        }
        self.query_count.fetch_add(1, AtomicOrdering::Relaxed);
        let mut store = self.store.write().await;
        let state = store
            .retention
            .entry(organization_id)
            .or_insert_with(|| initial_state(organization_id))
            .clone();
        validate_retained_query_window(state.records_available_from, filter, None)
            .map_err(RepositoryError::Conflict)?;
        let mut records = store
            .records
            .iter()
            .filter(|record| record.organization_id == organization_id)
            .filter(|record| {
                state
                    .records_available_from
                    .is_none_or(|boundary| record.occurred_at >= boundary)
            })
            .filter(|record| filter.matches(record))
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            right
                .occurred_at
                .cmp(&left.occurred_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        records.truncate(maximum_records);
        let snapshot = AuditExportSnapshot {
            retention_state: state,
            records,
        };
        snapshot
            .validate(organization_id, filter, maximum_records)
            .map_err(RepositoryError::Storage)?;
        Ok(snapshot)
    }

    async fn sweep_retention(
        &self,
        sweep: AuditRetentionSweep,
    ) -> Result<AuditRetentionReport, RepositoryError> {
        sweep.validate().map_err(RepositoryError::Storage)?;
        let mut store = self.store.write().await;
        let mut organizations = store
            .retention
            .values()
            .filter(|state| state.next_scan_at <= sweep.swept_at)
            .map(|state| (state.next_scan_at, state.organization_id))
            .collect::<Vec<_>>();
        organizations.sort_unstable();
        organizations.truncate(sweep.organization_batch_size);

        let mut report = AuditRetentionReport::default();
        let mut remaining_records = sweep.record_batch_size;
        for (_, organization_id) in organizations {
            if remaining_records == 0 {
                break;
            }
            report.inspected_organizations += 1;
            let current_boundary = store
                .retention
                .get(&organization_id)
                .and_then(|state| state.records_available_from);
            let boundary =
                current_boundary.map_or(sweep.cutoff, |current| current.max(sweep.cutoff));
            let mut candidates = store
                .records
                .iter()
                .filter(|record| {
                    record.organization_id == organization_id && record.occurred_at < boundary
                })
                .map(|record| (record.occurred_at, record.id))
                .collect::<Vec<_>>();
            candidates.sort_unstable();
            candidates.truncate(remaining_records);
            let candidate_ids = candidates
                .iter()
                .map(|(_, audit_id)| *audit_id)
                .collect::<BTreeSet<_>>();
            let current_state = store
                .retention
                .get(&organization_id)
                .expect("selected retention organization");
            let total_deleted_records = current_state
                .total_deleted_records
                .checked_add(candidate_ids.len() as u64)
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "audit retention deleted-record count overflowed".into(),
                    )
                })?;
            let version = current_state.version.checked_add(1).ok_or_else(|| {
                RepositoryError::Storage("audit retention version overflowed".into())
            })?;
            store
                .records
                .retain(|record| !candidate_ids.contains(&record.id));
            let completed = !store.records.iter().any(|record| {
                record.organization_id == organization_id && record.occurred_at < boundary
            });
            let state = store
                .retention
                .get_mut(&organization_id)
                .expect("selected retention organization");
            state.records_available_from = Some(boundary);
            if completed {
                state.records_deleted_before = Some(boundary);
                state.last_completed_at = Some(sweep.swept_at);
                report.completed_organizations += 1;
            }
            state.applied_policy_digest = Some(sweep.policy_digest.clone());
            state.total_deleted_records = total_deleted_records;
            state.last_swept_at = Some(sweep.swept_at);
            state.next_scan_at = sweep.next_scan_at;
            state.version = version;
            report.deleted_records += candidate_ids.len();
            remaining_records -= candidate_ids.len();
        }
        Ok(report)
    }
}

fn initial_state(organization_id: OrganizationId) -> AuditRetentionState {
    AuditRetentionState {
        organization_id,
        records_available_from: None,
        records_deleted_before: None,
        applied_policy_digest: None,
        total_deleted_records: 0,
        last_swept_at: None,
        last_completed_at: None,
        next_scan_at: DateTime::<Utc>::from_timestamp(0, 0).expect("Unix epoch"),
        version: 0,
    }
}
