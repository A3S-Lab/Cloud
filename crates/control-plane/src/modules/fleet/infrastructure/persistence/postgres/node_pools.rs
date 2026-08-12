use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, lock_idempotency_key, require_one_row, store_idempotency, store_outbox,
    transaction_error, PostgresPersistenceError,
};
use crate::modules::fleet::domain::entities::{NodePool, NodePoolMaintenanceWindow};
use crate::modules::fleet::domain::repositories::{NodeEvacuationCause, NodePoolWrite};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, NodeId, NodePoolId, OrganizationId, RepositoryError, ResourceName,
};
use a3s_orm::{
    sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, PostgresTransaction, Row,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct NodePoolRow {
    organization_id: Uuid,
    id: Uuid,
    name: String,
    spec_digest: String,
    aggregate_version: u64,
    maintenance_generation: u64,
    maintenance_starts_at: Option<DateTime<Utc>>,
    maintenance_ends_at: Option<DateTime<Utc>>,
    maintenance_reason: Option<String>,
    maintenance_cancelled_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl FromRow for NodePoolRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            id: decode(row, 1)?,
            name: decode(row, 2)?,
            spec_digest: decode(row, 3)?,
            aggregate_version: decode(row, 4)?,
            maintenance_generation: decode(row, 5)?,
            maintenance_starts_at: decode(row, 6)?,
            maintenance_ends_at: decode(row, 7)?,
            maintenance_reason: decode(row, 8)?,
            maintenance_cancelled_at: decode(row, 9)?,
            created_at: decode(row, 10)?,
            updated_at: decode(row, 11)?,
        })
    }
}

struct NodeIdRow(Uuid);

impl FromRow for NodeIdRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self(decode(row, 0)?))
    }
}

struct MaintenanceCauseRow {
    pool_id: Uuid,
    generation: u64,
    ends_at: DateTime<Utc>,
}

struct MaintenanceColumns {
    generation: u64,
    starts_at: Option<DateTime<Utc>>,
    ends_at: Option<DateTime<Utc>>,
    reason: Option<String>,
    cancelled_at: Option<DateTime<Utc>>,
}

impl FromRow for MaintenanceCauseRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            pool_id: decode(row, 0)?,
            generation: decode(row, 1)?,
            ends_at: decode(row, 2)?,
        })
    }
}

pub(super) async fn save(
    executor: &PostgresExecutor,
    write: NodePoolWrite,
) -> Result<IdempotentWrite<NodePool>, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                lock_idempotency_key(transaction, &write.idempotency).await?;
                if let Some(replayed) =
                    idempotency_replay::<NodePool>(transaction, &write.idempotency).await?
                {
                    replayed.value.validate().map_err(|error| {
                        PostgresPersistenceError::Invariant(format!(
                            "stored node pool idempotency response is invalid: {error}"
                        ))
                    })?;
                    return Ok(replayed);
                }
                validate_write(&write)?;
                match write.expected_version {
                    None => create(transaction, &write.pool).await?,
                    Some(expected_version) => {
                        update(transaction, &write.pool, expected_version).await?
                    }
                }
                store_outbox(transaction, &write.event).await?;
                store_idempotency(transaction, &write.idempotency, &write.pool).await?;
                Ok(IdempotentWrite {
                    value: write.pool,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn replay(
    executor: &PostgresExecutor,
    idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
) -> Result<Option<NodePool>, RepositoryError> {
    let idempotency = idempotency.clone();
    let replayed = executor
        .transaction(move |transaction| {
            Box::pin(async move { idempotency_replay::<NodePool>(transaction, &idempotency).await })
        })
        .await
        .map_err(transaction_error)?;
    if let Some(replayed) = replayed {
        replayed.value.validate().map_err(|error| {
            RepositoryError::Storage(format!(
                "stored node pool idempotency response is invalid: {error}"
            ))
        })?;
        Ok(Some(replayed.value))
    } else {
        Ok(None)
    }
}

pub(super) async fn find(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    pool_id: NodePoolId,
) -> Result<NodePool, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                find_in_transaction(transaction, organization_id, pool_id, false)
                    .await?
                    .ok_or_else(|| RepositoryError::NotFound.into())
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn list(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
) -> Result<Vec<NodePool>, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let rows = fetch_all(
                    transaction,
                    pool_select()
                        .append(" where organization_id = ")
                        .bind(organization_id.as_uuid())
                        .append(" order by name_key asc, id asc"),
                )
                .await?;
                let mut pools = Vec::with_capacity(rows.len());
                for row in rows {
                    pools.push(hydrate(transaction, row).await?);
                }
                Ok(pools)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn maintenance_cause(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    node_id: NodeId,
    evaluated_at: DateTime<Utc>,
) -> Result<Option<NodeEvacuationCause>, RepositoryError> {
    let result = executor
        .transaction(move |transaction| {
            Box::pin(async move {
                fetch_optional::<MaintenanceCauseRow, _>(
                    transaction,
                    sql_query::<MaintenanceCauseRow>(
                        "select p.id, p.maintenance_generation, p.maintenance_ends_at from node_pool_members m join node_pools p on p.organization_id = m.organization_id and p.id = m.node_pool_id join node_pool_maintenance_targets t on t.organization_id = m.organization_id and t.node_pool_id = m.node_pool_id and t.node_id = m.node_id where m.organization_id = ",
                    )
                    .bind(organization_id.as_uuid())
                    .append(" and m.node_id = ")
                    .bind(node_id.as_uuid())
                    .append(" and p.maintenance_generation > 0 and p.maintenance_cancelled_at is null and p.maintenance_starts_at <= ")
                    .bind(evaluated_at)
                    .append(" and p.maintenance_ends_at > ")
                    .bind(evaluated_at),
                )
                .await
            })
        })
        .await
        .map_err(transaction_error)?;
    Ok(result.map(|row| NodeEvacuationCause::PoolMaintenance {
        pool_id: NodePoolId::from_uuid(row.pool_id),
        generation: row.generation,
        ends_at: row.ends_at,
    }))
}

async fn create(
    transaction: &PostgresTransaction,
    pool: &NodePool,
) -> Result<(), PostgresPersistenceError> {
    if pool.aggregate_version != 1 || pool.maintenance.is_some() {
        return Err(RepositoryError::Conflict(
            "new node pool must start at version one without maintenance".into(),
        )
        .into());
    }
    let inserted = execute(transaction, insert_pool_query(pool)).await;
    match inserted {
        Ok(1) => {}
        Ok(rows) => {
            return Err(PostgresPersistenceError::Invariant(format!(
                "creating node pool affected {rows} rows"
            )))
        }
        Err(error) if is_unique_violation(&error) => {
            return Err(
                RepositoryError::Conflict("node pool name or ID already exists".into()).into(),
            )
        }
        Err(error) if is_foreign_key_violation(&error) => {
            return Err(RepositoryError::NotFound.into())
        }
        Err(error) => return Err(error),
    }
    for node_id in &pool.member_node_ids {
        insert_member(transaction, pool, *node_id).await?;
    }
    Ok(())
}

async fn update(
    transaction: &PostgresTransaction,
    pool: &NodePool,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let current = find_in_transaction(transaction, pool.organization_id, pool.id, true)
        .await?
        .ok_or(RepositoryError::NotFound)?;
    if current.aggregate_version != expected_version
        || pool.aggregate_version != expected_version.saturating_add(1)
        || pool.name != current.name
        || pool.created_at != current.created_at
        || current
            .member_node_ids
            .iter()
            .any(|node_id| pool.member_node_ids.binary_search(node_id).is_err())
    {
        return Err(RepositoryError::Conflict(
            "node pool aggregate version or additive membership changed".into(),
        )
        .into());
    }
    for node_id in pool
        .member_node_ids
        .iter()
        .filter(|node_id| current.member_node_ids.binary_search(node_id).is_err())
    {
        insert_member(transaction, pool, *node_id).await?;
    }
    let columns = maintenance_columns(pool);
    require_one_row(
        "node pool",
        execute(
            transaction,
            sql_query::<()>("update node_pools set spec_digest = ")
                .bind(pool.spec_digest.as_str())
                .append(", aggregate_version = ")
                .bind(pool.aggregate_version)
                .append(", maintenance_generation = ")
                .bind(columns.generation)
                .append(", maintenance_starts_at = ")
                .bind(columns.starts_at)
                .append(", maintenance_ends_at = ")
                .bind(columns.ends_at)
                .append(", maintenance_reason = ")
                .bind(columns.reason)
                .append(", maintenance_cancelled_at = ")
                .bind(columns.cancelled_at)
                .append(", updated_at = ")
                .bind(pool.updated_at)
                .append(" where organization_id = ")
                .bind(pool.organization_id.as_uuid())
                .append(" and id = ")
                .bind(pool.id.as_uuid())
                .append(" and aggregate_version = ")
                .bind(expected_version),
        )
        .await?,
    )?;
    execute(
        transaction,
        sql_query::<()>("delete from node_pool_maintenance_targets where organization_id = ")
            .bind(pool.organization_id.as_uuid())
            .append(" and node_pool_id = ")
            .bind(pool.id.as_uuid()),
    )
    .await?;
    if let Some(window) = &pool.maintenance {
        for node_id in &window.target_node_ids {
            require_one_row(
                "node pool maintenance target",
                execute(
                    transaction,
                    sql_query::<()>("insert into node_pool_maintenance_targets (organization_id, node_pool_id, node_id) values (")
                        .bind(pool.organization_id.as_uuid())
                        .append(", ")
                        .bind(pool.id.as_uuid())
                        .append(", ")
                        .bind(node_id.as_uuid())
                        .append(")"),
                )
                .await?,
            )?;
        }
    }
    Ok(())
}

async fn insert_member(
    transaction: &PostgresTransaction,
    pool: &NodePool,
    node_id: NodeId,
) -> Result<(), PostgresPersistenceError> {
    let inserted = execute(
        transaction,
        sql_query::<()>("insert into node_pool_members (organization_id, node_pool_id, node_id, joined_at) values (")
            .bind(pool.organization_id.as_uuid())
            .append(", ")
            .bind(pool.id.as_uuid())
            .append(", ")
            .bind(node_id.as_uuid())
            .append(", ")
            .bind(pool.updated_at)
            .append(")"),
    )
    .await;
    match inserted {
        Ok(rows) => require_one_row("node pool member", rows),
        Err(error) if is_unique_violation(&error) => {
            Err(RepositoryError::Conflict("node already belongs to a node pool".into()).into())
        }
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

async fn find_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    pool_id: NodePoolId,
    lock: bool,
) -> Result<Option<NodePool>, PostgresPersistenceError> {
    let mut query = pool_select()
        .append(" where organization_id = ")
        .bind(organization_id.as_uuid())
        .append(" and id = ")
        .bind(pool_id.as_uuid());
    if lock {
        query = query.append(" for update");
    }
    let Some(row) = fetch_optional(transaction, query).await? else {
        return Ok(None);
    };
    hydrate(transaction, row).await.map(Some)
}

async fn hydrate(
    transaction: &PostgresTransaction,
    row: NodePoolRow,
) -> Result<NodePool, PostgresPersistenceError> {
    let members = node_ids(
        transaction,
        "node_pool_members",
        row.organization_id,
        row.id,
    )
    .await?;
    let targets = node_ids(
        transaction,
        "node_pool_maintenance_targets",
        row.organization_id,
        row.id,
    )
    .await?;
    let maintenance = match row.maintenance_generation {
        0 => {
            if row.maintenance_starts_at.is_some()
                || row.maintenance_ends_at.is_some()
                || row.maintenance_reason.is_some()
                || row.maintenance_cancelled_at.is_some()
                || !targets.is_empty()
            {
                return Err(PostgresPersistenceError::Invariant(
                    "stored node pool has maintenance content without a generation".into(),
                ));
            }
            None
        }
        generation => Some(NodePoolMaintenanceWindow {
            generation,
            target_node_ids: targets,
            starts_at: row.maintenance_starts_at.ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "stored node pool maintenance start is missing".into(),
                )
            })?,
            ends_at: row.maintenance_ends_at.ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "stored node pool maintenance end is missing".into(),
                )
            })?,
            reason: row.maintenance_reason.ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "stored node pool maintenance reason is missing".into(),
                )
            })?,
            cancelled_at: row.maintenance_cancelled_at,
        }),
    };
    let name = ResourceName::parse(&row.name).map_err(|error| {
        PostgresPersistenceError::Invariant(format!("stored node pool name is invalid: {error}"))
    })?;
    if name.as_str() != row.name {
        return Err(PostgresPersistenceError::Invariant(
            "stored node pool name is not canonical".into(),
        ));
    }
    let pool = NodePool {
        id: NodePoolId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        name,
        member_node_ids: members,
        maintenance,
        spec_digest: row.spec_digest,
        aggregate_version: row.aggregate_version,
        created_at: row.created_at,
        updated_at: row.updated_at,
    };
    pool.validate().map_err(|error| {
        PostgresPersistenceError::Invariant(format!("stored node pool is invalid: {error}"))
    })?;
    Ok(pool)
}

async fn node_ids(
    transaction: &PostgresTransaction,
    table: &str,
    organization_id: Uuid,
    pool_id: Uuid,
) -> Result<Vec<NodeId>, PostgresPersistenceError> {
    let query = match table {
        "node_pool_members" => {
            sql_query::<NodeIdRow>("select node_id from node_pool_members where organization_id = ")
        }
        "node_pool_maintenance_targets" => sql_query::<NodeIdRow>(
            "select node_id from node_pool_maintenance_targets where organization_id = ",
        ),
        _ => {
            return Err(PostgresPersistenceError::Invariant(
                "unsupported node pool identity table".into(),
            ))
        }
    }
    .bind(organization_id)
    .append(" and node_pool_id = ")
    .bind(pool_id)
    .append(" order by node_id asc");
    fetch_all(transaction, query).await.map(|rows| {
        rows.into_iter()
            .map(|row| NodeId::from_uuid(row.0))
            .collect()
    })
}

fn validate_write(write: &NodePoolWrite) -> Result<(), PostgresPersistenceError> {
    write.pool.validate().map_err(|error| {
        RepositoryError::Conflict(format!("node pool specification is invalid: {error}"))
    })?;
    if write.event.organization_id != write.pool.organization_id.as_uuid()
        || write.event.aggregate_id != write.pool.id.as_uuid()
        || write.event.aggregate_version != write.pool.aggregate_version
    {
        return Err(PostgresPersistenceError::Invariant(
            "node pool event identity does not match the aggregate".into(),
        ));
    }
    Ok(())
}

fn insert_pool_query(pool: &NodePool) -> a3s_orm::SqlQuery<()> {
    let columns = maintenance_columns(pool);
    sql_query::<()>("insert into node_pools (organization_id, id, name, name_key, spec_digest, aggregate_version, maintenance_generation, maintenance_starts_at, maintenance_ends_at, maintenance_reason, maintenance_cancelled_at, created_at, updated_at) values (")
        .bind(pool.organization_id.as_uuid())
        .append(", ")
        .bind(pool.id.as_uuid())
        .append(", ")
        .bind(pool.name.as_str())
        .append(", ")
        .bind(pool.name.key())
        .append(", ")
        .bind(pool.spec_digest.as_str())
        .append(", ")
        .bind(pool.aggregate_version)
        .append(", ")
        .bind(columns.generation)
        .append(", ")
        .bind(columns.starts_at)
        .append(", ")
        .bind(columns.ends_at)
        .append(", ")
        .bind(columns.reason)
        .append(", ")
        .bind(columns.cancelled_at)
        .append(", ")
        .bind(pool.created_at)
        .append(", ")
        .bind(pool.updated_at)
        .append(")")
}

fn maintenance_columns(pool: &NodePool) -> MaintenanceColumns {
    pool.maintenance.as_ref().map_or(
        MaintenanceColumns {
            generation: 0,
            starts_at: None,
            ends_at: None,
            reason: None,
            cancelled_at: None,
        },
        |window| MaintenanceColumns {
            generation: window.generation,
            starts_at: Some(window.starts_at),
            ends_at: Some(window.ends_at),
            reason: Some(window.reason.clone()),
            cancelled_at: window.cancelled_at,
        },
    )
}

fn pool_select() -> a3s_orm::SqlQuery<NodePoolRow> {
    sql_query::<NodePoolRow>("select organization_id, id, name, spec_digest, aggregate_version, maintenance_generation, maintenance_starts_at, maintenance_ends_at, maintenance_reason, maintenance_cancelled_at, created_at, updated_at from node_pools")
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}
