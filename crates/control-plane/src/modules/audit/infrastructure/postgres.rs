use crate::infrastructure::AuditRecords;
use crate::modules::audit::domain::{
    AuditRecord, AuditRecordCursor, AuditRecordFilter, IAuditRecordRepository,
};
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, RepositoryError};
use a3s_orm::{select_from, Database, OrderDirection, PostgresDialect, PostgresExecutor};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

type AuditRecordRow = (Uuid, Uuid, Option<Uuid>, String, Uuid, DateTime<Utc>, Uuid);

#[derive(Clone)]
pub struct PostgresAuditRecordRepository {
    executor: PostgresExecutor,
}

impl PostgresAuditRecordRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IAuditRecordRepository for PostgresAuditRecordRepository {
    async fn list_page(
        &self,
        organization_id: OrganizationId,
        filter: &AuditRecordFilter,
        after: Option<AuditRecordCursor>,
        limit: usize,
    ) -> Result<Vec<AuditRecord>, RepositoryError> {
        let mut query = select_from::<AuditRecords>()
            .select((
                AuditRecords::audit_id(),
                AuditRecords::organization_id(),
                AuditRecords::actor_id(),
                AuditRecords::action(),
                AuditRecords::aggregate_id(),
                AuditRecords::occurred_at(),
                AuditRecords::request_id(),
            ))
            .filter(AuditRecords::organization_id().eq(organization_id.as_uuid()));
        if let Some(actor_id) = filter.actor_principal_id {
            query = query.filter(AuditRecords::actor_id().eq(Some(actor_id.as_uuid())));
        }
        if let Some(action) = &filter.action {
            query = query.filter(AuditRecords::action().eq(action.clone()));
        }
        if let Some(aggregate_id) = filter.aggregate_id {
            query = query.filter(AuditRecords::aggregate_id().eq(aggregate_id));
        }
        if let Some(request_id) = filter.request_id {
            query = query.filter(AuditRecords::request_id().eq(request_id));
        }
        if let Some(from) = filter.from {
            query = query.filter(AuditRecords::occurred_at().gte(from));
        }
        if let Some(to) = filter.to {
            query = query.filter(AuditRecords::occurred_at().lte(to));
        }
        if let Some(after) = after {
            query = query.filter(
                AuditRecords::occurred_at()
                    .lt(after.occurred_at)
                    .or(AuditRecords::occurred_at()
                        .eq(after.occurred_at)
                        .and(AuditRecords::audit_id().lt(after.audit_id))),
            );
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                query
                    .order_by(AuditRecords::occurred_at(), OrderDirection::Desc)
                    .order_by(AuditRecords::audit_id(), OrderDirection::Desc)
                    .limit(limit.max(1) as u64),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_record)
            .collect()
    }
}

fn decode_record(row: AuditRecordRow) -> Result<AuditRecord, RepositoryError> {
    let record = AuditRecord {
        id: row.0,
        organization_id: OrganizationId::from_uuid(row.1),
        actor_principal_id: row.2.map(PrincipalId::from_uuid),
        action: row.3,
        aggregate_id: row.4,
        occurred_at: row.5,
        request_id: row.6,
    };
    record.validate().map_err(RepositoryError::Storage)?;
    Ok(record)
}
