use super::postgres::insert_publication;
use super::postgres_gateway_scopes;
use super::postgres_schema::{GatewayRolloutReplicas, GatewayRollouts, GatewayScopes};
use super::postgres_tls::insert_certificate;
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_unique_violation, require_one_row,
    store_idempotency, store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::{GatewayRolloutResult, StageGatewayRollout};
use crate::modules::edge::domain::{
    GatewayReplicaRollout, GatewayReplicaRolloutState, GatewayRollout, GatewayRolloutPolicy,
    GatewayRolloutState, GatewayScopeState,
};
use crate::modules::shared_kernel::domain::{
    GatewayCertificateId, GatewayRolloutId, GatewayScopeId, NodeCommandId, NodeId, OrganizationId,
    RepositoryError,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    insert_into, select_from, sql_query, update_table, Database, DecodeError, Expression, FromRow,
    FromValue, OrderDirection, PostgresDialect, PostgresExecutor, Row,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_ROLLOUTS_FOR_UPDATE: &str = "select id, organization_id, gateway_scope_id, membership_generation, generation, correlation_id, min_ready, max_unavailable, desired_replicas, state, ready_replicas, unavailable_replicas, aggregate_version, started_at, completed_at from gateway_rollouts";
const SELECT_REPLICAS_FOR_UPDATE: &str = "select node_id, revision, command_id, snapshot_digest, snapshot_expires_at, gateway_certificate_id, state, failure, acknowledged_at from gateway_rollout_replicas";

#[derive(Debug, Clone, PartialEq, Eq)]
struct RolloutRow {
    id: Uuid,
    organization_id: Uuid,
    gateway_scope_id: Uuid,
    membership_generation: u64,
    generation: u64,
    correlation_id: Uuid,
    min_ready: u32,
    max_unavailable: u32,
    desired_replicas: u32,
    state: String,
    ready_replicas: u32,
    unavailable_replicas: u32,
    aggregate_version: u64,
    started_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

impl FromRow for RolloutRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Self::from_row_at(row, 0)
    }
}

impl RolloutRow {
    fn from_row_at(row: &impl Row, offset: usize) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, offset)?,
            organization_id: decode(row, offset + 1)?,
            gateway_scope_id: decode(row, offset + 2)?,
            membership_generation: decode(row, offset + 3)?,
            generation: decode(row, offset + 4)?,
            correlation_id: decode(row, offset + 5)?,
            min_ready: decode(row, offset + 6)?,
            max_unavailable: decode(row, offset + 7)?,
            desired_replicas: decode(row, offset + 8)?,
            state: decode(row, offset + 9)?,
            ready_replicas: decode(row, offset + 10)?,
            unavailable_replicas: decode(row, offset + 11)?,
            aggregate_version: decode(row, offset + 12)?,
            started_at: decode(row, offset + 13)?,
            completed_at: decode(row, offset + 14)?,
        })
    }
}

#[derive(Debug)]
struct ReplicaRow {
    node_id: Uuid,
    revision: u64,
    command_id: Uuid,
    snapshot_digest: String,
    snapshot_expires_at: DateTime<Utc>,
    gateway_certificate_id: Option<Uuid>,
    state: String,
    failure: Option<String>,
    acknowledged_at: Option<DateTime<Utc>>,
}

struct RolloutSelection;

impl Selection for RolloutSelection {
    type Output = RolloutRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            GatewayRollouts::id().expression(),
            GatewayRollouts::organization_id().expression(),
            GatewayRollouts::gateway_scope_id().expression(),
            GatewayRollouts::membership_generation().expression(),
            GatewayRollouts::generation().expression(),
            GatewayRollouts::correlation_id().expression(),
            GatewayRollouts::min_ready().expression(),
            GatewayRollouts::max_unavailable().expression(),
            GatewayRollouts::desired_replicas().expression(),
            GatewayRollouts::state().expression(),
            GatewayRollouts::ready_replicas().expression(),
            GatewayRollouts::unavailable_replicas().expression(),
            GatewayRollouts::aggregate_version().expression(),
            GatewayRollouts::started_at().expression(),
            GatewayRollouts::completed_at().expression(),
        ]
    }
}

struct ReplicaSelection;

impl Selection for ReplicaSelection {
    type Output = ReplicaRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            GatewayRolloutReplicas::node_id().expression(),
            GatewayRolloutReplicas::revision().expression(),
            GatewayRolloutReplicas::command_id().expression(),
            GatewayRolloutReplicas::snapshot_digest().expression(),
            GatewayRolloutReplicas::snapshot_expires_at().expression(),
            GatewayRolloutReplicas::gateway_certificate_id().expression(),
            GatewayRolloutReplicas::state().expression(),
            GatewayRolloutReplicas::failure().expression(),
            GatewayRolloutReplicas::acknowledged_at().expression(),
        ]
    }
}

struct RolloutReplicaSelection;

impl Selection for RolloutReplicaSelection {
    type Output = RolloutReplicaRow;

    fn expressions(self) -> Vec<Expression> {
        let mut expressions = RolloutSelection.expressions();
        expressions.extend(ReplicaSelection.expressions());
        expressions
    }
}

#[derive(Debug)]
struct RolloutReplicaRow {
    rollout: RolloutRow,
    replica: ReplicaRow,
}

impl FromRow for RolloutReplicaRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            rollout: RolloutRow::from_row_at(row, 0)?,
            replica: ReplicaRow::from_row_at(row, 15)?,
        })
    }
}

impl FromRow for ReplicaRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Self::from_row_at(row, 0)
    }
}

impl ReplicaRow {
    fn from_row_at(row: &impl Row, offset: usize) -> Result<Self, DecodeError> {
        Ok(Self {
            node_id: decode(row, offset)?,
            revision: decode(row, offset + 1)?,
            command_id: decode(row, offset + 2)?,
            snapshot_digest: decode(row, offset + 3)?,
            snapshot_expires_at: decode(row, offset + 4)?,
            gateway_certificate_id: decode(row, offset + 5)?,
            state: decode(row, offset + 6)?,
            failure: decode(row, offset + 7)?,
            acknowledged_at: decode(row, offset + 8)?,
        })
    }
}

impl ReplicaRow {
    fn replica(self) -> Result<GatewayReplicaRollout, RepositoryError> {
        Ok(GatewayReplicaRollout {
            node_id: NodeId::from_uuid(self.node_id),
            revision: self.revision,
            command_id: NodeCommandId::from_uuid(self.command_id),
            snapshot_digest: self.snapshot_digest,
            snapshot_expires_at: self.snapshot_expires_at,
            gateway_certificate_id: self
                .gateway_certificate_id
                .map(GatewayCertificateId::from_uuid),
            state: GatewayReplicaRolloutState::parse(&self.state)
                .map_err(RepositoryError::Storage)?,
            failure: self.failure,
            acknowledged_at: self.acknowledged_at,
        })
    }
}

impl RolloutRow {
    fn rollout(
        self,
        mut replicas: Vec<GatewayReplicaRollout>,
    ) -> Result<GatewayRollout, RepositoryError> {
        replicas.sort_by_key(|replica| replica.node_id);
        if usize::try_from(self.desired_replicas).ok() != Some(replicas.len()) {
            return Err(RepositoryError::Storage(
                "stored Gateway rollout desired replica count is inconsistent".into(),
            ));
        }
        let rollout = GatewayRollout {
            id: GatewayRolloutId::from_uuid(self.id),
            gateway_scope_id: GatewayScopeId::from_uuid(self.gateway_scope_id),
            membership_generation: self.membership_generation,
            generation: self.generation,
            correlation_id: self.correlation_id,
            policy: GatewayRolloutPolicy {
                min_ready: self.min_ready,
                max_unavailable: self.max_unavailable,
            },
            replicas,
            state: GatewayRolloutState::parse(&self.state).map_err(RepositoryError::Storage)?,
            ready_replicas: self.ready_replicas,
            unavailable_replicas: self.unavailable_replicas,
            aggregate_version: self.aggregate_version,
            started_at: self.started_at,
            completed_at: self.completed_at,
        };
        rollout.validate().map_err(RepositoryError::Storage)?;
        Ok(rollout)
    }
}

pub(super) async fn stage(
    executor: &PostgresExecutor,
    bundle: StageGatewayRollout,
) -> Result<GatewayRolloutResult, RepositoryError> {
    bundle.validate().map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(mut replay) =
                    idempotency_replay::<GatewayRolloutResult>(transaction, &bundle.idempotency)
                        .await?
                {
                    replay.value.replayed = true;
                    return Ok(replay.value);
                }
                let stored_scope =
                    postgres_gateway_scopes::load_for_share(transaction, bundle.scope.id)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                if stored_scope != bundle.scope {
                    return Err(RepositoryError::Conflict(
                        "Gateway scope changed while staging its rollout".into(),
                    )
                    .into());
                }
                if fetch_optional::<Uuid, _>(
                    transaction,
                    sql_query::<Uuid>("select id from gateway_rollouts where gateway_scope_id = ")
                        .bind(bundle.scope.id.as_uuid())
                        .append(" and state in ('pending', 'ready') for update"),
                )
                .await?
                .is_some()
                {
                    return Err(RepositoryError::Conflict(
                        "Gateway scope already has an active rollout".into(),
                    )
                    .into());
                }

                let mut physical_scopes = Vec::with_capacity(bundle.publications.len());
                for publication in &bundle.publications {
                    let current = lock_physical_scope(transaction, publication.node_id).await?;
                    let expected_version = bundle
                        .expected_scope_versions
                        .get(&publication.node_id)
                        .copied()
                        .ok_or_else(|| {
                            RepositoryError::Conflict(
                                "Gateway rollout omitted a physical scope version".into(),
                            )
                        })?;
                    if current.aggregate_version != expected_version {
                        return Err(RepositoryError::Conflict(
                            "physical Gateway scope changed while staging its rollout".into(),
                        )
                        .into());
                    }
                    if fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>("select 1 from gateway_publications where node_id = ")
                            .bind(publication.node_id.as_uuid())
                            .append(" and state = 'pending' for update"),
                    )
                    .await?
                    .is_some()
                    {
                        return Err(RepositoryError::Conflict(
                            "Gateway rollout member already has a pending complete snapshot".into(),
                        )
                        .into());
                    }
                    if publication.revision
                        != current.next_revision().map_err(RepositoryError::Conflict)?
                        || publication.expected_revision != current.installed_revision
                    {
                        return Err(RepositoryError::Conflict(
                            "Gateway rollout publication does not advance its physical revision"
                                .into(),
                        )
                        .into());
                    }
                    physical_scopes.push(current);
                }

                for publication in &bundle.publications {
                    insert_publication(transaction, publication).await?;
                }
                for certificate in &bundle.certificates {
                    insert_certificate(transaction, certificate).await?;
                }
                for (publication, current) in bundle.publications.iter().zip(physical_scopes.iter())
                {
                    advance_physical_scope(transaction, publication, current).await?;
                }
                insert_rollout(transaction, &bundle).await?;

                let mut publications = bundle.publications;
                publications.sort_by_key(|publication| publication.node_id);
                let mut certificates = bundle.certificates;
                certificates.sort_by_key(|certificate| certificate.node_id);
                let result = GatewayRolloutResult {
                    rollout: bundle.rollout,
                    publications,
                    certificates,
                    replayed: false,
                };
                store_outbox(transaction, &bundle.event).await?;
                store_idempotency(transaction, &bundle.idempotency, &result).await?;
                Ok(result)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn find(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
) -> Result<GatewayRollout, RepositoryError> {
    let database = Database::new(PostgresDialect, executor.clone());
    let rows = database
        .fetch_all_as(
            select_from::<GatewayRollouts>()
                .inner_join::<GatewayRolloutReplicas>(
                    GatewayRollouts::id().eq_column(GatewayRolloutReplicas::gateway_rollout_id()),
                )
                .select(RolloutReplicaSelection)
                .filter(GatewayRollouts::organization_id().eq(organization_id.as_uuid()))
                .filter(GatewayRollouts::id().eq(rollout_id.as_uuid()))
                .order_by(GatewayRolloutReplicas::node_id(), OrderDirection::Asc),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter();
    rebuild_rollout(rows)
}

pub(super) async fn mark_unavailable(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    expected_version: u64,
    failure: &str,
    observed_at: DateTime<Utc>,
) -> Result<GatewayRollout, RepositoryError> {
    let failure = failure.to_owned();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let (stored_organization_id, mut rollout) = lock_by_id(transaction, rollout_id)
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                if stored_organization_id != organization_id.as_uuid() {
                    return Err(RepositoryError::NotFound.into());
                }
                if rollout.aggregate_version != expected_version {
                    return Err(RepositoryError::Conflict(
                        "Gateway rollout changed before unavailability was recorded".into(),
                    )
                    .into());
                }
                rollout
                    .mark_unavailable(node_id, &failure, observed_at)
                    .map_err(RepositoryError::Conflict)?;
                persist_transition(transaction, &rollout, node_id, expected_version).await?;
                Ok(rollout)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn lock_by_gateway_identity(
    transaction: &a3s_orm::PostgresTransaction,
    node_id: Uuid,
    revision: u64,
    command_id: Uuid,
) -> Result<Option<GatewayRollout>, PostgresPersistenceError> {
    let rollout_id = fetch_optional::<Uuid, _>(
        transaction,
        select_from::<GatewayRolloutReplicas>()
            .select(GatewayRolloutReplicas::gateway_rollout_id())
            .filter(GatewayRolloutReplicas::node_id().eq(node_id))
            .filter(GatewayRolloutReplicas::revision().eq(revision))
            .filter(GatewayRolloutReplicas::command_id().eq(command_id)),
    )
    .await?;
    let Some(rollout_id) = rollout_id else {
        return Ok(None);
    };
    Ok(
        lock_by_id(transaction, GatewayRolloutId::from_uuid(rollout_id))
            .await?
            .map(|(_, rollout)| rollout),
    )
}

pub(super) async fn persist_acknowledgement(
    transaction: &a3s_orm::PostgresTransaction,
    rollout: &GatewayRollout,
    node_id: NodeId,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    persist_transition(transaction, rollout, node_id, expected_version).await
}

async fn lock_by_id(
    transaction: &a3s_orm::PostgresTransaction,
    rollout_id: GatewayRolloutId,
) -> Result<Option<(Uuid, GatewayRollout)>, PostgresPersistenceError> {
    let row = fetch_optional::<RolloutRow, _>(
        transaction,
        sql_query::<RolloutRow>(SELECT_ROLLOUTS_FOR_UPDATE)
            .append(" where id = ")
            .bind(rollout_id.as_uuid())
            .append(" for update"),
    )
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let organization_id = row.organization_id;
    let replicas = fetch_all::<ReplicaRow, _>(
        transaction,
        sql_query::<ReplicaRow>(SELECT_REPLICAS_FOR_UPDATE)
            .append(" where gateway_rollout_id = ")
            .bind(rollout_id.as_uuid())
            .append(" order by node_id for update"),
    )
    .await?
    .into_iter()
    .map(ReplicaRow::replica)
    .collect::<Result<Vec<_>, _>>()?;
    Ok(Some((organization_id, row.rollout(replicas)?)))
}

async fn lock_physical_scope(
    transaction: &a3s_orm::PostgresTransaction,
    node_id: NodeId,
) -> Result<GatewayScopeState, PostgresPersistenceError> {
    let scope = fetch_optional::<(u64, Option<u64>, u64), _>(
        transaction,
        sql_query::<(u64, Option<u64>, u64)>(
            "select last_issued_revision, installed_revision, aggregate_version from gateway_scopes where node_id = ",
        )
        .bind(node_id.as_uuid())
        .append(" for update"),
    )
    .await?;
    match scope {
        Some((last_issued_revision, installed_revision, aggregate_version))
            if last_issued_revision > 0
                && aggregate_version > 0
                && installed_revision
                    .is_none_or(|installed| installed > 0 && installed <= last_issued_revision) =>
        {
            Ok(GatewayScopeState {
                node_id,
                last_issued_revision,
                installed_revision,
                aggregate_version,
            })
        }
        Some(_) => Err(PostgresPersistenceError::Invariant(
            "stored physical Gateway scope is invalid".into(),
        )),
        None => Ok(GatewayScopeState::empty(node_id)),
    }
}

async fn advance_physical_scope(
    transaction: &a3s_orm::PostgresTransaction,
    publication: &crate::modules::edge::domain::GatewayPublication,
    current: &GatewayScopeState,
) -> Result<(), PostgresPersistenceError> {
    if current.aggregate_version == 0 {
        require_one_row(
            "physical Gateway scope",
            execute(
                transaction,
                insert_into::<GatewayScopes>()
                    .value(GatewayScopes::node_id(), publication.node_id.as_uuid())
                    .value(GatewayScopes::last_issued_revision(), publication.revision)
                    .value(
                        GatewayScopes::installed_revision(),
                        current.installed_revision,
                    )
                    .value(GatewayScopes::aggregate_version(), 1_u64)
                    .value(GatewayScopes::updated_at(), publication.command_issued_at),
            )
            .await?,
        )?;
    } else {
        let next_version = current.aggregate_version.checked_add(1).ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "physical Gateway scope aggregate version overflowed".into(),
            )
        })?;
        require_one_row(
            "physical Gateway scope",
            execute(
                transaction,
                update_table::<GatewayScopes>()
                    .set(GatewayScopes::last_issued_revision(), publication.revision)
                    .set(GatewayScopes::aggregate_version(), next_version)
                    .set(GatewayScopes::updated_at(), publication.command_issued_at)
                    .filter(GatewayScopes::node_id().eq(publication.node_id.as_uuid()))
                    .filter(GatewayScopes::aggregate_version().eq(current.aggregate_version)),
            )
            .await?,
        )?;
    }
    Ok(())
}

async fn insert_rollout(
    transaction: &a3s_orm::PostgresTransaction,
    bundle: &StageGatewayRollout,
) -> Result<(), PostgresPersistenceError> {
    let desired_replicas = u32::try_from(bundle.rollout.replicas.len()).map_err(|_| {
        PostgresPersistenceError::Invariant(
            "Gateway rollout desired replica count exceeds supported bounds".into(),
        )
    })?;
    let inserted = execute(
        transaction,
        insert_into::<GatewayRollouts>()
            .value(GatewayRollouts::id(), bundle.rollout.id.as_uuid())
            .value(
                GatewayRollouts::organization_id(),
                bundle.scope.organization_id.as_uuid(),
            )
            .value(
                GatewayRollouts::project_id(),
                bundle.scope.project_id.as_uuid(),
            )
            .value(
                GatewayRollouts::environment_id(),
                bundle.scope.environment_id.as_uuid(),
            )
            .value(
                GatewayRollouts::gateway_scope_id(),
                bundle.rollout.gateway_scope_id.as_uuid(),
            )
            .value(
                GatewayRollouts::membership_generation(),
                bundle.rollout.membership_generation,
            )
            .value(GatewayRollouts::generation(), bundle.rollout.generation)
            .value(
                GatewayRollouts::correlation_id(),
                bundle.rollout.correlation_id,
            )
            .value(
                GatewayRollouts::min_ready(),
                bundle.rollout.policy.min_ready,
            )
            .value(
                GatewayRollouts::max_unavailable(),
                bundle.rollout.policy.max_unavailable,
            )
            .value(GatewayRollouts::desired_replicas(), desired_replicas)
            .value(GatewayRollouts::state(), bundle.rollout.state.as_str())
            .value(
                GatewayRollouts::ready_replicas(),
                bundle.rollout.ready_replicas,
            )
            .value(
                GatewayRollouts::unavailable_replicas(),
                bundle.rollout.unavailable_replicas,
            )
            .value(
                GatewayRollouts::aggregate_version(),
                bundle.rollout.aggregate_version,
            )
            .value(GatewayRollouts::started_at(), bundle.rollout.started_at)
            .value(GatewayRollouts::completed_at(), bundle.rollout.completed_at),
    )
    .await;
    match inserted {
        Ok(rows) => require_one_row("Gateway rollout", rows)?,
        Err(error) if is_unique_violation(&error) => {
            return Err(RepositoryError::Conflict(
                "Gateway rollout identity, generation, or active slot already exists".into(),
            )
            .into())
        }
        Err(error) => return Err(error),
    }
    for replica in &bundle.rollout.replicas {
        require_one_row(
            "Gateway rollout replica",
            execute(
                transaction,
                insert_into::<GatewayRolloutReplicas>()
                    .value(
                        GatewayRolloutReplicas::gateway_rollout_id(),
                        bundle.rollout.id.as_uuid(),
                    )
                    .value(
                        GatewayRolloutReplicas::gateway_scope_id(),
                        bundle.rollout.gateway_scope_id.as_uuid(),
                    )
                    .value(
                        GatewayRolloutReplicas::membership_generation(),
                        bundle.rollout.membership_generation,
                    )
                    .value(GatewayRolloutReplicas::node_id(), replica.node_id.as_uuid())
                    .value(GatewayRolloutReplicas::revision(), replica.revision)
                    .value(
                        GatewayRolloutReplicas::command_id(),
                        replica.command_id.as_uuid(),
                    )
                    .value(
                        GatewayRolloutReplicas::snapshot_digest(),
                        replica.snapshot_digest.as_str(),
                    )
                    .value(
                        GatewayRolloutReplicas::snapshot_expires_at(),
                        replica.snapshot_expires_at,
                    )
                    .value(
                        GatewayRolloutReplicas::gateway_certificate_id(),
                        replica.gateway_certificate_id.map(|id| id.as_uuid()),
                    )
                    .value(GatewayRolloutReplicas::state(), replica.state.as_str())
                    .value(GatewayRolloutReplicas::failure(), replica.failure.clone())
                    .value(
                        GatewayRolloutReplicas::acknowledged_at(),
                        replica.acknowledged_at,
                    ),
            )
            .await?,
        )?;
    }
    Ok(())
}

async fn persist_transition(
    transaction: &a3s_orm::PostgresTransaction,
    rollout: &GatewayRollout,
    node_id: NodeId,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    rollout.validate().map_err(RepositoryError::Conflict)?;
    let replica = rollout
        .replicas
        .iter()
        .find(|replica| replica.node_id == node_id)
        .ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "Gateway rollout transition omitted its replica".into(),
            )
        })?;
    require_one_row(
        "Gateway rollout replica transition",
        execute(
            transaction,
            update_table::<GatewayRolloutReplicas>()
                .set(GatewayRolloutReplicas::state(), replica.state.as_str())
                .set(GatewayRolloutReplicas::failure(), replica.failure.clone())
                .set(
                    GatewayRolloutReplicas::acknowledged_at(),
                    replica.acknowledged_at,
                )
                .filter(GatewayRolloutReplicas::gateway_rollout_id().eq(rollout.id.as_uuid()))
                .filter(GatewayRolloutReplicas::node_id().eq(node_id.as_uuid()))
                .filter(GatewayRolloutReplicas::state().eq("pending")),
        )
        .await?,
    )?;
    require_one_row(
        "Gateway rollout transition",
        execute(
            transaction,
            update_table::<GatewayRollouts>()
                .set(GatewayRollouts::state(), rollout.state.as_str())
                .set(GatewayRollouts::ready_replicas(), rollout.ready_replicas)
                .set(
                    GatewayRollouts::unavailable_replicas(),
                    rollout.unavailable_replicas,
                )
                .set(
                    GatewayRollouts::aggregate_version(),
                    rollout.aggregate_version,
                )
                .set(GatewayRollouts::completed_at(), rollout.completed_at)
                .filter(GatewayRollouts::id().eq(rollout.id.as_uuid()))
                .filter(GatewayRollouts::aggregate_version().eq(expected_version)),
        )
        .await?,
    )?;
    Ok(())
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn rebuild_rollout(
    mut rows: impl Iterator<Item = RolloutReplicaRow>,
) -> Result<GatewayRollout, RepositoryError> {
    let first = rows.next().ok_or(RepositoryError::NotFound)?;
    let rollout = first.rollout;
    let mut replicas = vec![first.replica.replica()?];
    for row in rows {
        if row.rollout != rollout {
            return Err(RepositoryError::Storage(
                "joined Gateway rollout rows contain inconsistent aggregate data".into(),
            ));
        }
        replicas.push(row.replica.replica()?);
    }
    rollout.rollout(replicas)
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}
