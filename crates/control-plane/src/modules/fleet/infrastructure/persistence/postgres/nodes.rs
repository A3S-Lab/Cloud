use super::queries;
use super::rows::{self, NodeRow, SELECT_NODES};
use crate::infrastructure::{
    execute, idempotency_replay, require_one_row, store_idempotency, store_outbox,
    transaction_error, PostgresPersistenceError,
};
use crate::modules::fleet::domain::entities::Node;
use crate::modules::fleet::domain::repositories::{
    NodeEvacuationCause, NodeEvacuationSource, NodeHeartbeatUpdate,
};
use crate::modules::fleet::domain::value_objects::NodeState;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, IdempotencyRequest, IdempotentWrite, NodeId, NodePoolId, OrganizationId,
    RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor, PostgresTransaction};
use chrono::{DateTime, Utc};

pub(super) async fn record_heartbeat(
    executor: &PostgresExecutor,
    update: NodeHeartbeatUpdate,
) -> Result<Node, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move { record_heartbeat_in_transaction(transaction, update).await })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn record_heartbeat_in_transaction(
    transaction: &PostgresTransaction,
    mut update: NodeHeartbeatUpdate,
) -> Result<Node, PostgresPersistenceError> {
    update.observed_at = canonical_timestamp(update.observed_at);
    let mut node = queries::node_by_id(transaction, update.node_id, true)
        .await?
        .ok_or(RepositoryError::NotFound)?;
    if node.state == NodeState::Revoked {
        return Err(RepositoryError::NotFound.into());
    }
    if update.observed_at < node.last_observed_at {
        return Err(RepositoryError::Conflict("node heartbeat moved backwards".into()).into());
    }
    if update.observed_at == node.last_observed_at {
        if node.agent_instance_id != update.agent_instance_id
            || node.agent_version != update.agent_version
            || node.capabilities != update.capabilities
        {
            return Err(RepositoryError::Conflict(
                "node heartbeat timestamp was reused with different content".into(),
            )
            .into());
        }
        if node.state != NodeState::Pending {
            return Ok(node);
        }
    }
    let previous_version = node.aggregate_version;
    node.agent_instance_id = update.agent_instance_id;
    node.agent_version = update.agent_version;
    node.capabilities = update.capabilities;
    node.last_observed_at = update.observed_at;
    if node.state == NodeState::Pending {
        node.mark_ready().map_err(RepositoryError::Conflict)?;
    } else {
        node.aggregate_version += 1;
    }
    require_one_row(
        "node heartbeat",
        execute(
            transaction,
            sql_query::<()>("update nodes set state = ")
                .bind(node.state.as_str())
                .append(", agent_instance_id = ")
                .bind(node.agent_instance_id)
                .append(", agent_version = ")
                .bind(node.agent_version.as_str())
                .append(", runtime_provider_id = ")
                .bind(node.capabilities.provider_id())
                .append(", runtime_provider_build = ")
                .bind(node.capabilities.provider_build())
                .append(", capabilities_digest = ")
                .bind(node.capabilities.digest())
                .append(", capabilities = ")
                .bind(node.capabilities.document().clone())
                .append(", last_observed_at = ")
                .bind(node.last_observed_at)
                .append(", aggregate_version = ")
                .bind(node.aggregate_version)
                .append(" where organization_id = ")
                .bind(node.organization_id.as_uuid())
                .append(" and id = ")
                .bind(node.id.as_uuid())
                .append(" and aggregate_version = ")
                .bind(previous_version),
        )
        .await?,
    )?;
    Ok(node)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn set_state(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    node_id: NodeId,
    requested_state: NodeState,
    expected_version: u64,
    changed_at: DateTime<Utc>,
    event: DomainEventEnvelope,
    idempotency: IdempotencyRequest,
) -> Result<IdempotentWrite<Node>, RepositoryError> {
    let changed_at = canonical_timestamp(changed_at);
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replayed) =
                    idempotency_replay::<Node>(transaction, &idempotency).await?
                {
                    return Ok(replayed);
                }
                let mut node =
                    queries::node_by_identity(transaction, organization_id, node_id, true)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                if node.aggregate_version != expected_version {
                    return Err(
                        RepositoryError::Conflict("node aggregate version changed".into()).into(),
                    );
                }
                match requested_state {
                    NodeState::Ready => node.mark_ready(),
                    NodeState::Draining => node.drain(),
                    NodeState::Revoked => {
                        node.revoke();
                        Ok(())
                    }
                    NodeState::Pending => Err("node cannot transition back to pending".into()),
                }
                .map_err(RepositoryError::Conflict)?;
                require_one_row(
                    "node state",
                    execute(
                        transaction,
                        sql_query::<()>("update nodes set state = ")
                            .bind(node.state.as_str())
                            .append(", aggregate_version = ")
                            .bind(node.aggregate_version)
                            .append(" where organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(node_id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(expected_version),
                    )
                    .await?,
                )?;
                if requested_state == NodeState::Revoked {
                    let active = queries::active_certificate_by_node(transaction, node_id, true)
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "revoked node has no active certificate".into(),
                            )
                        })?;
                    require_one_row(
                        "revoked node certificate",
                        execute(
                            transaction,
                            sql_query::<()>("update node_certificates set revoked_at = ")
                                .bind(changed_at)
                                .append(" where id = ")
                                .bind(active.id.as_uuid())
                                .append(" and revoked_at is null"),
                        )
                        .await?,
                    )?;
                }
                if event.aggregate_version != node.aggregate_version {
                    return Err(PostgresPersistenceError::Invariant(
                        "node state event version does not match the node".into(),
                    ));
                }
                store_outbox(transaction, &event).await?;
                store_idempotency(transaction, &idempotency, &node).await?;
                Ok(IdempotentWrite {
                    value: node,
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
    node_id: NodeId,
) -> Result<Node, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                queries::node_by_identity(transaction, organization_id, node_id, false)
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
) -> Result<Vec<Node>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<NodeRow>(SELECT_NODES)
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" order by name_key asc, id asc"),
        )
        .await
        .map_err(|error| RepositoryError::Storage(error.to_string()))?
        .rows
        .into_iter()
        .map(rows::node)
        .collect()
}

pub(super) async fn list_scheduling_candidates(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    node_pool_id: Option<NodePoolId>,
    evaluated_at: DateTime<Utc>,
) -> Result<Vec<Node>, RepositoryError> {
    let mut query = sql_query::<NodeRow>(SELECT_NODES)
        .append(" where organization_id = ")
        .bind(organization_id.as_uuid());
    if let Some(node_pool_id) = node_pool_id {
        query = query
            .append(" and exists (select 1 from node_pool_members selected_pool where selected_pool.organization_id = nodes.organization_id and selected_pool.node_id = nodes.id and selected_pool.node_pool_id = ")
            .bind(node_pool_id.as_uuid())
            .append(")");
    }
    query = query
        .append(" and not ")
        .append(active_maintenance_exists())
        .bind(evaluated_at)
        .append(" and p.maintenance_ends_at > ")
        .bind(evaluated_at)
        .append(") order by name_key asc, id asc");
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(query)
        .await
        .map_err(|error| RepositoryError::Storage(error.to_string()))?
        .rows
        .into_iter()
        .map(rows::node)
        .collect()
}

pub(super) async fn list_evacuation_sources(
    executor: &PostgresExecutor,
    evaluated_at: DateTime<Utc>,
    limit: usize,
) -> Result<Vec<NodeEvacuationSource>, RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "node evacuation source query limit must be between 1 and 10000".into(),
        ));
    }
    let limit = u64::try_from(limit)
        .map_err(|_| RepositoryError::Conflict("node state query limit is invalid".into()))?;
    let nodes = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<NodeRow>(SELECT_NODES)
                .append(" where state = ")
                .bind(NodeState::Draining.as_str())
                .append(" or ")
                .append(active_maintenance_exists())
                .bind(evaluated_at)
                .append(" and p.maintenance_ends_at > ")
                .bind(evaluated_at)
                .append(")")
                .append(" order by organization_id asc, id asc limit ")
                .bind(limit),
        )
        .await
        .map_err(|error| RepositoryError::Storage(error.to_string()))?
        .rows
        .into_iter()
        .map(rows::node)
        .collect::<Result<Vec<_>, _>>()?;
    let mut sources = Vec::with_capacity(nodes.len());
    for node in nodes {
        let cause = if node.state == NodeState::Draining {
            Some(NodeEvacuationCause::ManualDrain)
        } else {
            super::node_pools::maintenance_cause(
                executor,
                node.organization_id,
                node.id,
                evaluated_at,
            )
            .await?
        };
        if let Some(cause) = cause {
            sources.push(NodeEvacuationSource { node, cause });
        }
    }
    Ok(sources)
}

pub(super) async fn find_evacuation_source(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    node_id: NodeId,
    evaluated_at: DateTime<Utc>,
) -> Result<NodeEvacuationSource, RepositoryError> {
    let node = find(executor, organization_id, node_id).await?;
    let cause = if node.state == NodeState::Draining {
        NodeEvacuationCause::ManualDrain
    } else {
        super::node_pools::maintenance_cause(executor, organization_id, node_id, evaluated_at)
            .await?
            .ok_or(RepositoryError::NotFound)?
    };
    Ok(NodeEvacuationSource { node, cause })
}

fn active_maintenance_exists() -> &'static str {
    "exists (select 1 from node_pool_members m join node_pools p on p.organization_id = m.organization_id and p.id = m.node_pool_id join node_pool_maintenance_targets t on t.organization_id = m.organization_id and t.node_pool_id = m.node_pool_id and t.node_id = m.node_id where m.organization_id = nodes.organization_id and m.node_id = nodes.id and p.maintenance_generation > 0 and p.maintenance_cancelled_at is null and p.maintenance_starts_at <= "
}
