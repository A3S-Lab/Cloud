use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_idempotency, store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::CreateGatewayScopeWrite;
use crate::modules::edge::domain::{GatewayRolloutPolicy, GatewayScope, Route};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GatewayScopeId, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    insert_into, select_from, Database, DecodeError, Expression, FromRow, FromValue,
    OrderDirection, PostgresDialect, PostgresExecutor, Row,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::postgres_schema::{GatewayRouteScopes, GatewayScopeMembers};

struct GatewayScopeRow {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    node_id: Uuid,
    membership_generation: u64,
    min_ready: u32,
    max_unavailable: u32,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct GatewayScopeSelection;

impl Selection for GatewayScopeSelection {
    type Output = GatewayScopeRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            GatewayRouteScopes::id().expression(),
            GatewayRouteScopes::organization_id().expression(),
            GatewayRouteScopes::project_id().expression(),
            GatewayRouteScopes::environment_id().expression(),
            GatewayRouteScopes::node_id().expression(),
            GatewayRouteScopes::membership_generation().expression(),
            GatewayRouteScopes::min_ready().expression(),
            GatewayRouteScopes::max_unavailable().expression(),
            GatewayRouteScopes::aggregate_version().expression(),
            GatewayRouteScopes::created_at().expression(),
            GatewayRouteScopes::updated_at().expression(),
        ]
    }
}

impl FromRow for GatewayScopeRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            project_id: decode(row, 2)?,
            environment_id: decode(row, 3)?,
            node_id: decode(row, 4)?,
            membership_generation: decode(row, 5)?,
            min_ready: decode(row, 6)?,
            max_unavailable: decode(row, 7)?,
            aggregate_version: decode(row, 8)?,
            created_at: decode(row, 9)?,
            updated_at: decode(row, 10)?,
        })
    }
}

impl GatewayScopeRow {
    fn scope(self, member_node_ids: Vec<Uuid>) -> Result<GatewayScope, RepositoryError> {
        let member_node_ids = member_node_ids
            .into_iter()
            .map(crate::modules::shared_kernel::domain::NodeId::from_uuid)
            .collect::<Vec<_>>();
        let scope = GatewayScope {
            id: GatewayScopeId::from_uuid(self.id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            node_id: crate::modules::shared_kernel::domain::NodeId::from_uuid(self.node_id),
            member_node_ids,
            membership_generation: self.membership_generation,
            rollout_policy: GatewayRolloutPolicy {
                min_ready: self.min_ready,
                max_unavailable: self.max_unavailable,
            },
            aggregate_version: self.aggregate_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
        };
        validate_stored(&scope)?;
        Ok(scope)
    }
}

pub(super) async fn create(
    executor: &PostgresExecutor,
    bundle: CreateGatewayScopeWrite,
) -> Result<IdempotentWrite<GatewayScope>, RepositoryError> {
    bundle.scope.validate().map_err(RepositoryError::Conflict)?;
    validate_event(&bundle)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) =
                    idempotency_replay::<GatewayScope>(transaction, &bundle.idempotency).await?
                {
                    return Ok(replay);
                }
                let inserted = execute(
                    transaction,
                    insert_into::<GatewayRouteScopes>()
                        .value(GatewayRouteScopes::id(), bundle.scope.id.as_uuid())
                        .value(
                            GatewayRouteScopes::organization_id(),
                            bundle.scope.organization_id.as_uuid(),
                        )
                        .value(
                            GatewayRouteScopes::project_id(),
                            bundle.scope.project_id.as_uuid(),
                        )
                        .value(
                            GatewayRouteScopes::environment_id(),
                            bundle.scope.environment_id.as_uuid(),
                        )
                        .value(
                            GatewayRouteScopes::node_id(),
                            bundle.scope.node_id.as_uuid(),
                        )
                        .value(
                            GatewayRouteScopes::membership_generation(),
                            bundle.scope.membership_generation,
                        )
                        .value(
                            GatewayRouteScopes::min_ready(),
                            bundle.scope.rollout_policy.min_ready,
                        )
                        .value(
                            GatewayRouteScopes::max_unavailable(),
                            bundle.scope.rollout_policy.max_unavailable,
                        )
                        .value(
                            GatewayRouteScopes::aggregate_version(),
                            bundle.scope.aggregate_version,
                        )
                        .value(GatewayRouteScopes::created_at(), bundle.scope.created_at)
                        .value(GatewayRouteScopes::updated_at(), bundle.scope.updated_at),
                )
                .await;
                match inserted {
                    Ok(rows) => require_one_row("Gateway scope", rows)?,
                    Err(error) if is_unique_violation(&error) => {
                        return Err(RepositoryError::Conflict(
                            "Gateway node is already bound to this environment scope".into(),
                        )
                        .into())
                    }
                    Err(error) if is_foreign_key_violation(&error) => {
                        return Err(RepositoryError::NotFound.into())
                    }
                    Err(error) => return Err(error),
                }
                for (ordinal, node_id) in bundle.scope.member_node_ids.iter().enumerate() {
                    let inserted = execute(
                        transaction,
                        insert_into::<GatewayScopeMembers>()
                            .value(
                                GatewayScopeMembers::gateway_scope_id(),
                                bundle.scope.id.as_uuid(),
                            )
                            .value(
                                GatewayScopeMembers::organization_id(),
                                bundle.scope.organization_id.as_uuid(),
                            )
                            .value(
                                GatewayScopeMembers::project_id(),
                                bundle.scope.project_id.as_uuid(),
                            )
                            .value(
                                GatewayScopeMembers::environment_id(),
                                bundle.scope.environment_id.as_uuid(),
                            )
                            .value(GatewayScopeMembers::node_id(), node_id.as_uuid())
                            .value(
                                GatewayScopeMembers::ordinal(),
                                u32::try_from(ordinal).map_err(|_| {
                                    PostgresPersistenceError::Invariant(
                                        "Gateway scope member ordinal exceeds supported bounds"
                                            .into(),
                                    )
                                })?,
                            )
                            .value(
                                GatewayScopeMembers::membership_generation(),
                                bundle.scope.membership_generation,
                            )
                            .value(GatewayScopeMembers::added_at(), bundle.scope.created_at),
                    )
                    .await;
                    match inserted {
                        Ok(rows) => require_one_row("Gateway scope member", rows)?,
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Gateway node is already bound to this environment scope".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                }
                store_outbox(transaction, &bundle.event).await?;
                store_idempotency(transaction, &bundle.idempotency, &bundle.scope).await?;
                Ok(IdempotentWrite {
                    value: bundle.scope,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn find(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    scope_id: GatewayScopeId,
) -> Result<GatewayScope, RepositoryError> {
    let database = Database::new(PostgresDialect, executor.clone());
    let row = database
        .fetch_optional_as(
            select_from::<GatewayRouteScopes>()
                .select(GatewayScopeSelection)
                .filter(GatewayRouteScopes::organization_id().eq(organization_id.as_uuid()))
                .filter(GatewayRouteScopes::id().eq(scope_id.as_uuid())),
        )
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::NotFound)?;
    let member_node_ids = database
        .fetch_all_as(
            select_from::<GatewayScopeMembers>()
                .select(GatewayScopeMembers::node_id())
                .filter(GatewayScopeMembers::gateway_scope_id().eq(scope_id.as_uuid()))
                .order_by(GatewayScopeMembers::ordinal(), OrderDirection::Asc),
        )
        .await
        .map_err(storage)?
        .rows;
    row.scope(member_node_ids)
}

pub(super) async fn list(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
) -> Result<Vec<GatewayScope>, RepositoryError> {
    let database = Database::new(PostgresDialect, executor.clone());
    let rows = database
        .fetch_all_as(
            select_from::<GatewayRouteScopes>()
                .select(GatewayScopeSelection)
                .filter(GatewayRouteScopes::organization_id().eq(organization_id.as_uuid()))
                .filter(GatewayRouteScopes::project_id().eq(project_id.as_uuid()))
                .filter(GatewayRouteScopes::environment_id().eq(environment_id.as_uuid()))
                .order_by(GatewayRouteScopes::created_at(), OrderDirection::Asc)
                .order_by(GatewayRouteScopes::id(), OrderDirection::Asc),
        )
        .await
        .map_err(storage)?
        .rows;
    let mut members = database
        .fetch_all_as(
            select_from::<GatewayScopeMembers>()
                .select((
                    GatewayScopeMembers::gateway_scope_id(),
                    GatewayScopeMembers::node_id(),
                ))
                .filter(GatewayScopeMembers::organization_id().eq(organization_id.as_uuid()))
                .filter(GatewayScopeMembers::project_id().eq(project_id.as_uuid()))
                .filter(GatewayScopeMembers::environment_id().eq(environment_id.as_uuid()))
                .order_by(GatewayScopeMembers::gateway_scope_id(), OrderDirection::Asc)
                .order_by(GatewayScopeMembers::ordinal(), OrderDirection::Asc),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .fold(
            std::collections::BTreeMap::<Uuid, Vec<Uuid>>::new(),
            |mut grouped, (scope_id, node_id)| {
                grouped.entry(scope_id).or_default().push(node_id);
                grouped
            },
        );
    rows.into_iter()
        .map(|row| {
            let member_node_ids = members.remove(&row.id).unwrap_or_default();
            row.scope(member_node_ids)
        })
        .collect()
}

fn validate_event(bundle: &CreateGatewayScopeWrite) -> Result<(), RepositoryError> {
    let scope = &bundle.scope;
    let event = &bundle.event;
    if scope.aggregate_version != 1
        || scope.updated_at != scope.created_at
        || event.event_key != "edge.gateway-scope.created"
        || event.schema_version != 2
        || event.organization_id != scope.organization_id.as_uuid()
        || event.aggregate_id != scope.id.as_uuid()
        || event.aggregate_version != scope.aggregate_version
        || event.occurred_at != scope.created_at
        || event.correlation_id.is_nil()
        || event.event_id.is_nil()
    {
        return Err(RepositoryError::Conflict(
            "Gateway scope event does not match its aggregate".into(),
        ));
    }
    Ok(())
}

fn validate_stored(scope: &GatewayScope) -> Result<(), RepositoryError> {
    scope.validate().map_err(RepositoryError::Storage)
}

pub(super) async fn validate_route_binding(
    transaction: &a3s_orm::PostgresTransaction,
    expected: &GatewayScope,
    route: &Route,
) -> Result<(), PostgresPersistenceError> {
    let stored = load_for_share(transaction, route.gateway_scope_id).await?;
    if stored.as_ref() != Some(expected)
        || !expected.owns(
            route.organization_id,
            route.project_id,
            route.environment_id,
            route.gateway_node_id,
        )
    {
        return Err(RepositoryError::Conflict(
            "route does not belong to the selected Gateway scope".into(),
        )
        .into());
    }
    Ok(())
}

pub(super) async fn validate_cutover_bindings(
    transaction: &a3s_orm::PostgresTransaction,
    routes: &[Route],
) -> Result<(), PostgresPersistenceError> {
    for route in routes {
        let scope = load_for_share(transaction, route.gateway_scope_id)
            .await?
            .ok_or(RepositoryError::NotFound)?;
        if !scope.owns(
            route.organization_id,
            route.project_id,
            route.environment_id,
            route.gateway_node_id,
        ) {
            return Err(RepositoryError::Conflict(
                "route cutover crossed its Gateway scope boundary".into(),
            )
            .into());
        }
    }
    Ok(())
}

pub(super) async fn load_for_share(
    transaction: &a3s_orm::PostgresTransaction,
    scope_id: GatewayScopeId,
) -> Result<Option<GatewayScope>, PostgresPersistenceError> {
    let row = fetch_optional::<GatewayScopeRow, _>(
        transaction,
        select_from::<GatewayRouteScopes>()
            .select(GatewayScopeSelection)
            .filter(GatewayRouteScopes::id().eq(scope_id.as_uuid()))
            .for_share(),
    )
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let member_node_ids = crate::infrastructure::fetch_all::<Uuid, _>(
        transaction,
        select_from::<GatewayScopeMembers>()
            .select(GatewayScopeMembers::node_id())
            .filter(GatewayScopeMembers::gateway_scope_id().eq(scope_id.as_uuid()))
            .order_by(GatewayScopeMembers::ordinal(), OrderDirection::Asc),
    )
    .await?;
    row.scope(member_node_ids)
        .map(Some)
        .map_err(PostgresPersistenceError::from)
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}
