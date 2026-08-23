use crate::infrastructure::{AuditRecords, OutboxEvents};
use crate::modules::edge::domain::events::{
    MCP_ROUTE_POLICY_CREATED_EVENT_KEY, MCP_ROUTE_POLICY_REVISED_EVENT_KEY,
};
use crate::modules::security::domain::{
    GatewayRoutePolicyTimelineCursor, GatewayRoutePolicyTimelineEntry,
    IGatewayRoutePolicyTimelineRepository,
};
use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError, RouteId};
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_orm::{
    bound, coalesce, select_from, sql_function, Database, OrderDirection, PostgresDialect,
    PostgresExecutor,
};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashSet;
use uuid::Uuid;

type TimelineRow = (Value, Uuid, Option<Uuid>);

#[derive(Clone)]
pub struct PostgresGatewayRoutePolicyTimelineRepository {
    executor: PostgresExecutor,
}

impl PostgresGatewayRoutePolicyTimelineRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IGatewayRoutePolicyTimelineRepository for PostgresGatewayRoutePolicyTimelineRepository {
    async fn list_page(
        &self,
        organization_id: OrganizationId,
        route_id: RouteId,
        after: Option<GatewayRoutePolicyTimelineCursor>,
        limit: usize,
    ) -> Result<Vec<GatewayRoutePolicyTimelineEntry>, RepositoryError> {
        let audit_join = AuditRecords::organization_id()
            .eq_column(OutboxEvents::organization_id())
            .and(AuditRecords::aggregate_id().eq_column(OutboxEvents::aggregate_id()))
            .and(AuditRecords::action().eq_column(OutboxEvents::event_key()))
            .and(AuditRecords::occurred_at().eq_column(OutboxEvents::occurred_at()))
            .and(AuditRecords::request_id().eq_column(OutboxEvents::correlation_id()));
        let audit_id = coalesce::<Uuid>([
            AuditRecords::audit_id().expression(),
            bound::<Uuid>(Uuid::nil()).expression(),
        ]);
        let event_document = sql_function::<Value>(
            "jsonb_build_object",
            [
                bound::<String>("event_id").expression(),
                OutboxEvents::event_id().expression(),
                bound::<String>("event_key").expression(),
                OutboxEvents::event_key().expression(),
                bound::<String>("schema_version").expression(),
                OutboxEvents::schema_version().expression(),
                bound::<String>("organization_id").expression(),
                OutboxEvents::organization_id().expression(),
                bound::<String>("aggregate_id").expression(),
                OutboxEvents::aggregate_id().expression(),
                bound::<String>("aggregate_version").expression(),
                OutboxEvents::aggregate_version().expression(),
                bound::<String>("occurred_at").expression(),
                OutboxEvents::occurred_at().expression(),
                bound::<String>("correlation_id").expression(),
                OutboxEvents::correlation_id().expression(),
                bound::<String>("causation_id").expression(),
                OutboxEvents::causation_id().expression(),
                bound::<String>("payload").expression(),
                OutboxEvents::payload().expression(),
            ],
        );
        let mut query = select_from::<OutboxEvents>()
            .left_join::<AuditRecords>(audit_join)
            .select((event_document, audit_id, AuditRecords::actor_id()))
            .filter(OutboxEvents::organization_id().eq(organization_id.as_uuid()))
            .filter(OutboxEvents::aggregate_id().eq(route_id.as_uuid()))
            .filter(
                OutboxEvents::event_key()
                    .eq(MCP_ROUTE_POLICY_CREATED_EVENT_KEY)
                    .or(OutboxEvents::event_key().eq(MCP_ROUTE_POLICY_REVISED_EVENT_KEY)),
            );
        if let Some(after) = after {
            query = query.filter(
                OutboxEvents::occurred_at()
                    .lt(after.occurred_at)
                    .or(OutboxEvents::occurred_at()
                        .eq(after.occurred_at)
                        .and(OutboxEvents::event_id().lt(after.event_id))),
            );
        }
        let rows: Vec<TimelineRow> = Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                query
                    .order_by(OutboxEvents::occurred_at(), OrderDirection::Desc)
                    .order_by(OutboxEvents::event_id(), OrderDirection::Desc)
                    .limit(limit.max(1) as u64),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows;
        let mut seen = HashSet::with_capacity(rows.len());
        rows.into_iter()
            .map(|row| {
                let entry = decode_entry(row)?;
                if !seen.insert(entry.event_id) {
                    return Err(RepositoryError::Storage(
                        "security timeline owner fact has ambiguous audit correlation".into(),
                    ));
                }
                Ok(entry)
            })
            .collect()
    }
}

fn decode_entry(row: TimelineRow) -> Result<GatewayRoutePolicyTimelineEntry, RepositoryError> {
    let event: DomainEventEnvelope = serde_json::from_value(row.0).map_err(|error| {
        RepositoryError::Storage(format!(
            "security timeline event document is invalid: {error}"
        ))
    })?;
    let audit_record_id = (!row.1.is_nil()).then_some(row.1);
    GatewayRoutePolicyTimelineEntry::from_owner_event(&event, audit_record_id, row.2)
        .map_err(RepositoryError::Storage)
}
