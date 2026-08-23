use crate::infrastructure::AuditRecords;
use crate::modules::audit::domain::{
    AuditAttributionStatus, AuditRecord, AuditRecordCursor, AuditRecordFilter,
    IAuditRecordRepository,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, PrincipalId, ProjectAttributionProfileId, ProjectId,
    RepositoryError,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    select_from, Database, DecodeError, Expression, FromRow, FromValue, OrderDirection,
    PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct AuditRecordRow {
    id: Uuid,
    organization_id: Uuid,
    actor_principal_id: Option<Uuid>,
    action: String,
    aggregate_id: Uuid,
    occurred_at: DateTime<Utc>,
    request_id: Uuid,
    project_id: Option<Uuid>,
    environment_id: Option<Uuid>,
    attribution_profile_id: Option<Uuid>,
    attribution_status: String,
}

impl FromRow for AuditRecordRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode_column(row, 0)?,
            organization_id: decode_column(row, 1)?,
            actor_principal_id: decode_column(row, 2)?,
            action: decode_column(row, 3)?,
            aggregate_id: decode_column(row, 4)?,
            occurred_at: decode_column(row, 5)?,
            request_id: decode_column(row, 6)?,
            project_id: decode_column(row, 7)?,
            environment_id: decode_column(row, 8)?,
            attribution_profile_id: decode_column(row, 9)?,
            attribution_status: decode_column(row, 10)?,
        })
    }
}

struct AuditRecordSelection;

impl Selection for AuditRecordSelection {
    type Output = AuditRecordRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            AuditRecords::audit_id().expression(),
            AuditRecords::organization_id().expression(),
            AuditRecords::actor_id().expression(),
            AuditRecords::action().expression(),
            AuditRecords::aggregate_id().expression(),
            AuditRecords::occurred_at().expression(),
            AuditRecords::request_id().expression(),
            AuditRecords::project_id().expression(),
            AuditRecords::environment_id().expression(),
            AuditRecords::attribution_profile_id().expression(),
            AuditRecords::attribution_status().expression(),
        ]
    }
}

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
            .select(AuditRecordSelection)
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
        if let Some(project_id) = filter.project_id {
            query = query.filter(AuditRecords::project_id().eq(Some(project_id.as_uuid())));
        }
        if let Some(environment_id) = filter.environment_id {
            query = query.filter(AuditRecords::environment_id().eq(Some(environment_id.as_uuid())));
        }
        if let Some(profile_id) = filter.attribution_profile_id {
            query =
                query.filter(AuditRecords::attribution_profile_id().eq(Some(profile_id.as_uuid())));
        }
        if let Some(status) = filter.attribution_status {
            query = query.filter(AuditRecords::attribution_status().eq(status.as_str()));
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
        id: row.id,
        organization_id: OrganizationId::from_uuid(row.organization_id),
        actor_principal_id: row.actor_principal_id.map(PrincipalId::from_uuid),
        action: row.action,
        aggregate_id: row.aggregate_id,
        occurred_at: row.occurred_at,
        request_id: row.request_id,
        project_id: row.project_id.map(ProjectId::from_uuid),
        environment_id: row.environment_id.map(EnvironmentId::from_uuid),
        attribution_profile_id: row
            .attribution_profile_id
            .map(ProjectAttributionProfileId::from_uuid),
        attribution_status: AuditAttributionStatus::parse(&row.attribution_status)
            .map_err(RepositoryError::Storage)?,
    };
    record.validate().map_err(RepositoryError::Storage)?;
    Ok(record)
}

fn decode_column<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}
