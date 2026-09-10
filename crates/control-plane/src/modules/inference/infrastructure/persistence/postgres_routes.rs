use super::postgres_routes_schema::InferenceRoutes;
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, require_one_row, store_idempotency, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::inference::domain::entities::InferenceRoute;
use crate::modules::inference::domain::repositories::{
    InferenceRouteWriteReference, PublishInferenceRouteWrite, RetireInferenceRouteWrite,
    IInferenceRouteRepository,
};
use crate::modules::inference::domain::value_objects::EdgeRouteBindingRef;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayScopeId, IdempotencyRequest, IdempotentWrite,
    InferenceRouteId, OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::{InferenceGrantAclProjection, InferenceModelAclProjection};
use a3s_orm::expression::Selection;
use a3s_orm::{
    insert_into, select_from, update_table, DecodeError, Expression, FromRow, FromValue,
    OrderDirection, PostgresExecutor, PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Durable Inference route catalog backed by PostgreSQL.
#[derive(Clone)]
pub struct PostgresInferenceRouteRepository {
    executor: PostgresExecutor,
}

impl PostgresInferenceRouteRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IInferenceRouteRepository for PostgresInferenceRouteRepository {
    async fn find_inference_route(
        &self,
        organization_id: OrganizationId,
        route_id: InferenceRouteId,
    ) -> Result<Option<InferenceRoute>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let row = fetch_optional::<InferenceRouteRow, _>(
                        transaction,
                        select_from::<InferenceRoutes>()
                            .select(InferenceRouteSelection)
                            .filter(
                                InferenceRoutes::organization_id().eq(organization_id.as_uuid()),
                            )
                            .filter(InferenceRoutes::id().eq(route_id.as_uuid())),
                    )
                    .await?;
                    Ok(match row {
                        Some(row) => Some(row.into_route()?),
                        None => None,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_active_inference_routes_by_environment(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<InferenceRoute>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let rows = fetch_all::<InferenceRouteRow, _>(
                        transaction,
                        select_from::<InferenceRoutes>()
                            .select(InferenceRouteSelection)
                            .filter(
                                InferenceRoutes::organization_id().eq(organization_id.as_uuid()),
                            )
                            .filter(InferenceRoutes::project_id().eq(project_id.as_uuid()))
                            .filter(
                                InferenceRoutes::environment_id().eq(environment_id.as_uuid()),
                            )
                            .filter(InferenceRoutes::retired_at().is_null())
                            .order_by(InferenceRoutes::id(), OrderDirection::Asc),
                    )
                    .await?;
                    let mut routes = Vec::with_capacity(rows.len());
                    for row in rows {
                        routes.push(row.into_route()?);
                    }
                    Ok(routes)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn replay_inference_route_write(
        &self,
        organization_id: OrganizationId,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<InferenceRoute>>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move { replay(transaction, organization_id, &idempotency).await })
            })
            .await
            .map_err(transaction_error)
    }

    async fn publish_inference_route(
        &self,
        write: PublishInferenceRouteWrite,
    ) -> Result<IdempotentWrite<InferenceRoute>, RepositoryError> {
        write.validate().map_err(RepositoryError::Conflict)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) =
                        replay(transaction, write.route.organization_id, &write.idempotency).await?
                    {
                        return Ok(replayed);
                    }
                    insert_route(transaction, &write.route).await?;
                    store_idempotency(
                        transaction,
                        &write.idempotency,
                        &InferenceRouteWriteReference::from_route(&write.route),
                    )
                    .await?;
                    Ok(IdempotentWrite {
                        value: write.route,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn retire_inference_route(
        &self,
        write: RetireInferenceRouteWrite,
    ) -> Result<IdempotentWrite<InferenceRoute>, RepositoryError> {
        write.validate().map_err(RepositoryError::Conflict)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) =
                        replay(transaction, write.route.organization_id, &write.idempotency).await?
                    {
                        return Ok(replayed);
                    }
                    let existing = fetch_optional::<InferenceRouteRow, _>(
                        transaction,
                        select_from::<InferenceRoutes>()
                            .select(InferenceRouteSelection)
                            .filter(
                                InferenceRoutes::organization_id()
                                    .eq(write.route.organization_id.as_uuid()),
                            )
                            .filter(InferenceRoutes::id().eq(write.route.id.as_uuid())),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?
                    .into_route()?;
                    if write.is_noop() {
                        if existing != write.route {
                            return Err(RepositoryError::Conflict(
                                "inference route changed while applying retirement".into(),
                            )
                            .into());
                        }
                    } else {
                        write
                            .route
                            .validate_transition_from(&existing, write.expected_aggregate_version)
                            .map_err(RepositoryError::Conflict)?;
                        update_route_row(
                            transaction,
                            &write.route,
                            write.expected_aggregate_version,
                        )
                        .await?;
                    }
                    store_idempotency(
                        transaction,
                        &write.idempotency,
                        &InferenceRouteWriteReference::from_route(&write.route),
                    )
                    .await?;
                    Ok(IdempotentWrite {
                        value: write.route,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}

async fn replay(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    idempotency: &IdempotencyRequest,
) -> Result<Option<IdempotentWrite<InferenceRoute>>, PostgresPersistenceError> {
    let Some(IdempotentWrite {
        value: reference,
        replayed: _,
    }) = idempotency_replay::<InferenceRouteWriteReference>(transaction, idempotency).await?
    else {
        return Ok(None);
    };
    if reference.organization_id != organization_id {
        return Err(RepositoryError::Storage(
            "stored inference route idempotency reference is invalid".into(),
        )
        .into());
    }
    let route = fetch_optional::<InferenceRouteRow, _>(
        transaction,
        select_from::<InferenceRoutes>()
            .select(InferenceRouteSelection)
            .filter(InferenceRoutes::organization_id().eq(organization_id.as_uuid()))
            .filter(InferenceRoutes::id().eq(reference.route_id.as_uuid())),
    )
    .await?
    .ok_or_else(|| {
        RepositoryError::Storage("inference route idempotency target is missing".into())
    })?
    .into_route()?;
    if route.aggregate_version() != reference.aggregate_version {
        return Err(RepositoryError::Storage(
            "inference route idempotency target changed after write".into(),
        )
        .into());
    }
    Ok(Some(IdempotentWrite {
        value: route,
        replayed: true,
    }))
}

async fn insert_route(
    transaction: &PostgresTransaction,
    route: &InferenceRoute,
) -> Result<(), PostgresPersistenceError> {
    let models = serde_json::to_value(route.models())
        .map_err(|error| RepositoryError::Storage(error.to_string()))?;
    let grants = serde_json::to_value(route.grants())
        .map_err(|error| RepositoryError::Storage(error.to_string()))?;
    let binding = route.binding();
    let result = execute(
        transaction,
        insert_into::<InferenceRoutes>()
            .value(InferenceRoutes::id(), route.id.as_uuid())
            .value(
                InferenceRoutes::organization_id(),
                route.organization_id.as_uuid(),
            )
            .value(InferenceRoutes::project_id(), route.project_id.as_uuid())
            .value(
                InferenceRoutes::environment_id(),
                route.environment_id.as_uuid(),
            )
            .value(InferenceRoutes::router(), route.router())
            .value(InferenceRoutes::policy_revision(), route.policy_revision())
            .value(InferenceRoutes::models(), models)
            .value(InferenceRoutes::grants(), grants)
            .value(
                InferenceRoutes::domain_claim_id(),
                binding.domain_claim_id.as_uuid(),
            )
            .value(
                InferenceRoutes::gateway_scope_id(),
                binding.gateway_scope_id.as_uuid(),
            )
            .value(InferenceRoutes::hostname(), binding.hostname.as_str())
            .value(InferenceRoutes::path_prefix(), binding.path_prefix.as_str())
            .value(
                InferenceRoutes::binding_generation(),
                binding.binding_generation,
            )
            .value(
                InferenceRoutes::aggregate_version(),
                route.aggregate_version(),
            )
            .value(InferenceRoutes::created_at(), route.created_at())
            .value(InferenceRoutes::updated_at(), route.updated_at())
            .value(InferenceRoutes::retired_at(), route.retired_at()),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("inference route", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "inference route identity is already in use".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

async fn update_route_row(
    transaction: &PostgresTransaction,
    route: &InferenceRoute,
    expected_aggregate_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let models = serde_json::to_value(route.models())
        .map_err(|error| RepositoryError::Storage(error.to_string()))?;
    let grants = serde_json::to_value(route.grants())
        .map_err(|error| RepositoryError::Storage(error.to_string()))?;
    let binding = route.binding();
    let result = execute(
        transaction,
        update_table::<InferenceRoutes>()
            .set(InferenceRoutes::router(), route.router())
            .set(InferenceRoutes::policy_revision(), route.policy_revision())
            .set(InferenceRoutes::models(), models)
            .set(InferenceRoutes::grants(), grants)
            .set(
                InferenceRoutes::domain_claim_id(),
                binding.domain_claim_id.as_uuid(),
            )
            .set(
                InferenceRoutes::gateway_scope_id(),
                binding.gateway_scope_id.as_uuid(),
            )
            .set(InferenceRoutes::hostname(), binding.hostname.as_str())
            .set(InferenceRoutes::path_prefix(), binding.path_prefix.as_str())
            .set(
                InferenceRoutes::binding_generation(),
                binding.binding_generation,
            )
            .set(
                InferenceRoutes::aggregate_version(),
                route.aggregate_version(),
            )
            .set(InferenceRoutes::updated_at(), route.updated_at())
            .set(InferenceRoutes::retired_at(), route.retired_at())
            .filter(InferenceRoutes::organization_id().eq(route.organization_id.as_uuid()))
            .filter(InferenceRoutes::id().eq(route.id.as_uuid()))
            .filter(InferenceRoutes::aggregate_version().eq(expected_aggregate_version)),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("inference route update", rows),
        Err(error) => Err(error),
    }
}

struct InferenceRouteRow {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    router: String,
    policy_revision: u64,
    models: serde_json::Value,
    grants: serde_json::Value,
    domain_claim_id: Uuid,
    gateway_scope_id: Uuid,
    hostname: String,
    path_prefix: String,
    binding_generation: u64,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    retired_at: Option<DateTime<Utc>>,
}

struct InferenceRouteSelection;

impl Selection for InferenceRouteSelection {
    type Output = InferenceRouteRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            InferenceRoutes::id().expression(),
            InferenceRoutes::organization_id().expression(),
            InferenceRoutes::project_id().expression(),
            InferenceRoutes::environment_id().expression(),
            InferenceRoutes::router().expression(),
            InferenceRoutes::policy_revision().expression(),
            InferenceRoutes::models().expression(),
            InferenceRoutes::grants().expression(),
            InferenceRoutes::domain_claim_id().expression(),
            InferenceRoutes::gateway_scope_id().expression(),
            InferenceRoutes::hostname().expression(),
            InferenceRoutes::path_prefix().expression(),
            InferenceRoutes::binding_generation().expression(),
            InferenceRoutes::aggregate_version().expression(),
            InferenceRoutes::created_at().expression(),
            InferenceRoutes::updated_at().expression(),
            InferenceRoutes::retired_at().expression(),
        ]
    }
}

impl FromRow for InferenceRouteRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            project_id: decode(row, 2)?,
            environment_id: decode(row, 3)?,
            router: decode(row, 4)?,
            policy_revision: decode(row, 5)?,
            models: decode(row, 6)?,
            grants: decode(row, 7)?,
            domain_claim_id: decode(row, 8)?,
            gateway_scope_id: decode(row, 9)?,
            hostname: decode(row, 10)?,
            path_prefix: decode(row, 11)?,
            binding_generation: decode(row, 12)?,
            aggregate_version: decode(row, 13)?,
            created_at: decode(row, 14)?,
            updated_at: decode(row, 15)?,
            retired_at: decode(row, 16)?,
        })
    }
}

impl InferenceRouteRow {
    fn into_route(self) -> Result<InferenceRoute, RepositoryError> {
        let models: Vec<InferenceModelAclProjection> = serde_json::from_value(self.models)
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        let grants: Vec<InferenceGrantAclProjection> = serde_json::from_value(self.grants)
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        let binding = EdgeRouteBindingRef::new(
            DomainClaimId::from_uuid(self.domain_claim_id),
            GatewayScopeId::from_uuid(self.gateway_scope_id),
            self.hostname,
            self.path_prefix,
            self.binding_generation,
        )
        .map_err(RepositoryError::Storage)?;
        InferenceRoute::restore(
            InferenceRouteId::from_uuid(self.id),
            OrganizationId::from_uuid(self.organization_id),
            ProjectId::from_uuid(self.project_id),
            EnvironmentId::from_uuid(self.environment_id),
            self.router,
            self.policy_revision,
            models,
            grants,
            binding,
            self.aggregate_version,
            self.created_at,
            self.updated_at,
            self.retired_at,
        )
        .map_err(RepositoryError::Storage)
    }
}

fn decode<T>(row: &impl Row, index: usize) -> Result<T, DecodeError>
where
    T: FromValue,
{
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}
