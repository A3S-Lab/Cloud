use crate::infrastructure::{
    execute, fetch_all, fetch_optional, require_one_row, store_outbox, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::fleet::domain::entities::Node;
use crate::modules::fleet::domain::events::{
    node_availability_phase_version, NodeAvailabilityChanged, NodeAvailabilityFactStatus,
    NodeAvailabilityFiring, NodeAvailabilityResolutionReason, NodeAvailabilitySnapshot,
    NODE_UNAVAILABLE_EVENT_KEY,
};
use crate::modules::fleet::domain::repositories::{
    NodeAvailabilityReconciliationResult, ReconcileNodeAvailability,
};
use crate::modules::fleet::domain::value_objects::NodeState;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, NodeId, OrganizationId, RepositoryError,
};
use a3s_orm::{
    sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, PostgresTransaction, Row,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AvailabilityHeadState {
    Observed,
    Unavailable,
    Resolved,
}

impl AvailabilityHeadState {
    fn parse(value: &str) -> Result<Self, PostgresPersistenceError> {
        match value {
            "observed" => Ok(Self::Observed),
            "unavailable" => Ok(Self::Unavailable),
            "resolved" => Ok(Self::Resolved),
            _ => Err(PostgresPersistenceError::Invariant(format!(
                "stored Node availability fact-head state {value:?} is invalid"
            ))),
        }
    }
}

struct AvailabilityNodeRow {
    organization_id: Uuid,
    node_id: Uuid,
    state: String,
    last_observed_at: DateTime<Utc>,
    node_aggregate_version: u64,
}

impl FromRow for AvailabilityNodeRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            node_id: decode(row, 1)?,
            state: decode(row, 2)?,
            last_observed_at: decode(row, 3)?,
            node_aggregate_version: decode(row, 4)?,
        })
    }
}

impl AvailabilityNodeRow {
    fn snapshot(self) -> Result<NodeAvailabilitySnapshot, PostgresPersistenceError> {
        let state = NodeState::parse(&self.state).map_err(PostgresPersistenceError::Invariant)?;
        let snapshot = NodeAvailabilitySnapshot {
            organization_id: OrganizationId::from_uuid(self.organization_id),
            node_id: NodeId::from_uuid(self.node_id),
            state,
            node_aggregate_version: self.node_aggregate_version,
            last_observed_at: self.last_observed_at,
        };
        if snapshot.organization_id.as_uuid().is_nil()
            || snapshot.node_id.as_uuid().is_nil()
            || snapshot.node_aggregate_version == 0
            || canonical_timestamp(snapshot.last_observed_at) != snapshot.last_observed_at
            || !matches!(snapshot.state, NodeState::Ready | NodeState::Draining)
        {
            return Err(PostgresPersistenceError::Invariant(
                "Node availability candidate is inconsistent".into(),
            ));
        }
        Ok(snapshot)
    }
}

struct AvailabilityHeadRow {
    state: String,
    node_aggregate_version: u64,
    last_observed_at: DateTime<Utc>,
    timeout_deadline_at: Option<DateTime<Utc>>,
    firing_event_id: Option<Uuid>,
    firing_phase_version: Option<u64>,
    firing_node_aggregate_version: Option<u64>,
    firing_last_observed_at: Option<DateTime<Utc>>,
    firing_timeout_deadline_at: Option<DateTime<Utc>>,
    detected_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
}

impl FromRow for AvailabilityHeadRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            state: decode(row, 0)?,
            node_aggregate_version: decode(row, 1)?,
            last_observed_at: decode(row, 2)?,
            timeout_deadline_at: decode(row, 3)?,
            firing_event_id: decode(row, 4)?,
            firing_phase_version: decode(row, 5)?,
            firing_node_aggregate_version: decode(row, 6)?,
            firing_last_observed_at: decode(row, 7)?,
            firing_timeout_deadline_at: decode(row, 8)?,
            detected_at: decode(row, 9)?,
            updated_at: decode(row, 10)?,
        })
    }
}

impl AvailabilityHeadRow {
    fn parsed_state(&self) -> Result<AvailabilityHeadState, PostgresPersistenceError> {
        AvailabilityHeadState::parse(&self.state)
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}

pub(super) async fn reconcile(
    executor: &PostgresExecutor,
    mut request: ReconcileNodeAvailability,
) -> Result<NodeAvailabilityReconciliationResult, RepositoryError> {
    request.evaluated_at = canonical_timestamp(request.evaluated_at);
    if request.heartbeat_timeout <= chrono::Duration::zero()
        || request.limit == 0
        || request.limit > 10_000
    {
        return Err(RepositoryError::Conflict(
            "Node availability reconciliation request is invalid".into(),
        ));
    }
    executor
        .transaction(move |transaction| {
            Box::pin(async move { reconcile_transaction(transaction, request).await })
        })
        .await
        .map_err(transaction_error)
}

async fn reconcile_transaction(
    transaction: &PostgresTransaction,
    request: ReconcileNodeAvailability,
) -> Result<NodeAvailabilityReconciliationResult, PostgresPersistenceError> {
    let limit = u64::try_from(request.limit).map_err(|_| {
        PostgresPersistenceError::Invariant(
            "Node availability reconciliation limit exceeds supported range".into(),
        )
    })?;
    let rows = fetch_all::<AvailabilityNodeRow, _>(
        transaction,
        sql_query::<AvailabilityNodeRow>(
            "select n.organization_id, n.id, n.state, n.last_observed_at, n.aggregate_version from nodes n left join fleet_node_availability_fact_heads h on h.organization_id = n.organization_id and h.node_id = n.id where n.state in ('ready', 'draining') and (h.node_id is null or (h.state in ('observed', 'resolved') and (h.timeout_deadline_at is null or ",
        )
        .bind(request.evaluated_at)
        .append(" > h.timeout_deadline_at))) order by n.organization_id asc, n.id asc limit ")
        .bind(limit)
        .append(" for update of n skip locked"),
    )
    .await?;

    let mut result = NodeAvailabilityReconciliationResult {
        processed_nodes: rows.len(),
        ..NodeAvailabilityReconciliationResult::default()
    };
    for row in rows {
        let snapshot = row.snapshot()?;
        let head = fact_head(
            transaction,
            snapshot.organization_id,
            snapshot.node_id,
            true,
        )
        .await?;
        match head {
            None => {
                result.initialized_heads += 1;
                let deadline = availability_deadline(snapshot, request.heartbeat_timeout)?;
                insert_observed_head(
                    transaction,
                    snapshot,
                    deadline,
                    monotonic_time(request.evaluated_at, snapshot.last_observed_at),
                )
                .await?;
            }
            Some(head) => {
                validate_current_head(snapshot, &head)?;
                if head.parsed_state()? == AvailabilityHeadState::Unavailable {
                    return Err(PostgresPersistenceError::Invariant(
                        "unavailable Node fact head was selected for another firing".into(),
                    ));
                }
                let deadline = match head.timeout_deadline_at {
                    Some(deadline) => deadline,
                    None => availability_deadline(snapshot, request.heartbeat_timeout)?,
                };
                if request.evaluated_at > deadline {
                    let detected_at = monotonic_time(request.evaluated_at, head.updated_at);
                    let event = NodeAvailabilityChanged::unavailable_envelope(
                        snapshot,
                        deadline,
                        detected_at,
                    )
                    .map_err(PostgresPersistenceError::Invariant)?;
                    fire_unavailable_head(transaction, snapshot, &head, deadline, &event).await?;
                    store_outbox(transaction, &event).await?;
                    result.unavailable_facts += 1;
                } else if head.timeout_deadline_at.is_none() {
                    anchor_deadline(
                        transaction,
                        snapshot,
                        &head,
                        deadline,
                        monotonic_time(request.evaluated_at, head.updated_at),
                    )
                    .await?;
                } else {
                    return Err(PostgresPersistenceError::Invariant(
                        "fresh Node fact head was selected for reconciliation".into(),
                    ));
                }
            }
        }
    }
    Ok(result)
}

fn availability_deadline(
    snapshot: NodeAvailabilitySnapshot,
    heartbeat_timeout: chrono::Duration,
) -> Result<DateTime<Utc>, PostgresPersistenceError> {
    snapshot
        .last_observed_at
        .checked_add_signed(heartbeat_timeout)
        .map(canonical_timestamp)
        .ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "Node availability deadline exceeds supported time".into(),
            )
        })
}

fn monotonic_time(candidate: DateTime<Utc>, floor: DateTime<Utc>) -> DateTime<Utc> {
    canonical_timestamp(if candidate < floor { floor } else { candidate })
}

fn validate_current_head(
    snapshot: NodeAvailabilitySnapshot,
    head: &AvailabilityHeadRow,
) -> Result<(), PostgresPersistenceError> {
    if head.node_aggregate_version == 0
        || head.node_aggregate_version > snapshot.node_aggregate_version
        || head.last_observed_at != snapshot.last_observed_at
        || canonical_timestamp(head.updated_at) != head.updated_at
    {
        return Err(PostgresPersistenceError::Invariant(
            "Node availability fact head diverged from its owner observation".into(),
        ));
    }
    Ok(())
}

async fn fact_head(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    node_id: NodeId,
    lock: bool,
) -> Result<Option<AvailabilityHeadRow>, PostgresPersistenceError> {
    let mut query = sql_query::<AvailabilityHeadRow>(
        "select state, node_aggregate_version, last_observed_at, timeout_deadline_at, firing_event_id, firing_phase_version, firing_node_aggregate_version, firing_last_observed_at, firing_timeout_deadline_at, detected_at, updated_at from fleet_node_availability_fact_heads where organization_id = ",
    )
    .bind(organization_id.as_uuid())
    .append(" and node_id = ")
    .bind(node_id.as_uuid());
    if lock {
        query = query.append(" for update");
    }
    fetch_optional(transaction, query).await
}

async fn insert_observed_head(
    transaction: &PostgresTransaction,
    snapshot: NodeAvailabilitySnapshot,
    deadline: DateTime<Utc>,
    updated_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Node availability observation head",
        execute(
            transaction,
            sql_query::<()>(
                "insert into fleet_node_availability_fact_heads (organization_id, node_id, state, node_aggregate_version, last_observed_at, timeout_deadline_at, updated_at) values (",
            )
            .bind(snapshot.organization_id.as_uuid())
            .append(", ")
            .bind(snapshot.node_id.as_uuid())
            .append(", 'observed', ")
            .bind(snapshot.node_aggregate_version)
            .append(", ")
            .bind(snapshot.last_observed_at)
            .append(", ")
            .bind(deadline)
            .append(", ")
            .bind(updated_at)
            .append(")"),
        )
        .await?,
    )
}

async fn anchor_deadline(
    transaction: &PostgresTransaction,
    snapshot: NodeAvailabilitySnapshot,
    head: &AvailabilityHeadRow,
    deadline: DateTime<Utc>,
    updated_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Node availability deadline anchor",
        execute(
            transaction,
            sql_query::<()>(
                "update fleet_node_availability_fact_heads set node_aggregate_version = ",
            )
            .bind(snapshot.node_aggregate_version)
            .append(", timeout_deadline_at = ")
            .bind(deadline)
            .append(", updated_at = ")
            .bind(updated_at)
            .append(" where organization_id = ")
            .bind(snapshot.organization_id.as_uuid())
            .append(" and node_id = ")
            .bind(snapshot.node_id.as_uuid())
            .append(" and state = ")
            .bind(head.state.as_str())
            .append(" and last_observed_at = ")
            .bind(head.last_observed_at)
            .append(" and timeout_deadline_at is null"),
        )
        .await?,
    )
}

async fn fire_unavailable_head(
    transaction: &PostgresTransaction,
    snapshot: NodeAvailabilitySnapshot,
    head: &AvailabilityHeadRow,
    deadline: DateTime<Utc>,
    event: &a3s_cloud_contracts::DomainEventEnvelope,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Node unavailable fact transition",
        execute(
            transaction,
            sql_query::<()>(
                "update fleet_node_availability_fact_heads set state = 'unavailable', node_aggregate_version = ",
            )
            .bind(snapshot.node_aggregate_version)
            .append(", timeout_deadline_at = ")
            .bind(deadline)
            .append(", latest_event_id = ")
            .bind(event.event_id)
            .append(", latest_event_key = ")
            .bind(event.event_key.as_str())
            .append(", latest_phase_version = ")
            .bind(event.aggregate_version)
            .append(", firing_event_id = ")
            .bind(event.event_id)
            .append(", firing_phase_version = ")
            .bind(event.aggregate_version)
            .append(", firing_node_aggregate_version = ")
            .bind(snapshot.node_aggregate_version)
            .append(", firing_last_observed_at = ")
            .bind(snapshot.last_observed_at)
            .append(", firing_timeout_deadline_at = ")
            .bind(deadline)
            .append(", detected_at = ")
            .bind(event.occurred_at)
            .append(", resolved_at = null, resolution_reason = null, updated_at = ")
            .bind(event.occurred_at)
            .append(" where organization_id = ")
            .bind(snapshot.organization_id.as_uuid())
            .append(" and node_id = ")
            .bind(snapshot.node_id.as_uuid())
            .append(" and state = ")
            .bind(head.state.as_str())
            .append(" and last_observed_at = ")
            .bind(head.last_observed_at),
        )
        .await?,
    )
}

pub(super) async fn record_heartbeat_transition(
    transaction: &PostgresTransaction,
    previous: &Node,
    current: &Node,
    received_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    if current.last_observed_at <= previous.last_observed_at {
        return Ok(());
    }
    let Some(head) = fact_head(transaction, current.organization_id, current.id, true).await?
    else {
        return Ok(());
    };
    if head.last_observed_at != previous.last_observed_at
        || head.node_aggregate_version > previous.aggregate_version
    {
        return Err(PostgresPersistenceError::Invariant(
            "heartbeat does not advance the current Node availability fact head".into(),
        ));
    }
    let received_at = monotonic_time(received_at, head.updated_at);
    match head.parsed_state()? {
        AvailabilityHeadState::Observed | AvailabilityHeadState::Resolved => {
            advance_observation_head(transaction, current, &head, received_at).await
        }
        AvailabilityHeadState::Unavailable => {
            let firing = firing_from_head(current.organization_id, current.id, &head)?;
            let resolved_at = monotonic_time(received_at, firing.detected_at);
            let event = NodeAvailabilityChanged::resolved_envelope(
                NodeAvailabilitySnapshot::from_node(current),
                firing,
                NodeAvailabilityResolutionReason::HeartbeatRestored,
                resolved_at,
            )
            .map_err(PostgresPersistenceError::Invariant)?;
            resolve_head(
                transaction,
                current,
                &head,
                NodeAvailabilityResolutionReason::HeartbeatRestored,
                &event,
            )
            .await?;
            store_outbox(transaction, &event).await
        }
    }
}

pub(super) async fn record_revoke_transition(
    transaction: &PostgresTransaction,
    current: &Node,
    changed_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let Some(head) = fact_head(transaction, current.organization_id, current.id, true).await?
    else {
        return Ok(());
    };
    if head.parsed_state()? != AvailabilityHeadState::Unavailable {
        return Ok(());
    }
    if head.last_observed_at != current.last_observed_at
        || head.node_aggregate_version >= current.aggregate_version
    {
        return Err(PostgresPersistenceError::Invariant(
            "revocation does not advance the open Node availability firing".into(),
        ));
    }
    let firing = firing_from_head(current.organization_id, current.id, &head)?;
    let resolved_at = monotonic_time(changed_at, firing.detected_at);
    let event = NodeAvailabilityChanged::resolved_envelope(
        NodeAvailabilitySnapshot::from_node(current),
        firing,
        NodeAvailabilityResolutionReason::NodeRevoked,
        resolved_at,
    )
    .map_err(PostgresPersistenceError::Invariant)?;
    resolve_head(
        transaction,
        current,
        &head,
        NodeAvailabilityResolutionReason::NodeRevoked,
        &event,
    )
    .await?;
    store_outbox(transaction, &event).await
}

fn firing_from_head(
    organization_id: OrganizationId,
    node_id: NodeId,
    head: &AvailabilityHeadRow,
) -> Result<NodeAvailabilityFiring, PostgresPersistenceError> {
    let firing = NodeAvailabilityFiring {
        organization_id,
        node_id,
        event_id: head.firing_event_id.ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "unavailable Node fact head omitted its firing event".into(),
            )
        })?,
        phase_version: head.firing_phase_version.ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "unavailable Node fact head omitted its phase".into(),
            )
        })?,
        node_aggregate_version: head.firing_node_aggregate_version.ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "unavailable Node fact head omitted its Node version".into(),
            )
        })?,
        last_observed_at: head.firing_last_observed_at.ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "unavailable Node fact head omitted its observation".into(),
            )
        })?,
        timeout_deadline_at: head.firing_timeout_deadline_at.ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "unavailable Node fact head omitted its deadline".into(),
            )
        })?,
        detected_at: head.detected_at.ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "unavailable Node fact head omitted its detection time".into(),
            )
        })?,
    };
    let expected_phase = node_availability_phase_version(
        firing.node_aggregate_version,
        NodeAvailabilityFactStatus::Unavailable,
    )
    .map_err(PostgresPersistenceError::Invariant)?;
    if firing.phase_version != expected_phase
        || firing.event_id
            != NodeAvailabilityChanged::deterministic_event_id(
                node_id,
                NODE_UNAVAILABLE_EVENT_KEY,
                expected_phase,
            )
    {
        return Err(PostgresPersistenceError::Invariant(
            "unavailable Node fact-head identity is inconsistent".into(),
        ));
    }
    Ok(firing)
}

async fn advance_observation_head(
    transaction: &PostgresTransaction,
    current: &Node,
    head: &AvailabilityHeadRow,
    updated_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Node availability heartbeat cursor",
        execute(
            transaction,
            sql_query::<()>(
                "update fleet_node_availability_fact_heads set node_aggregate_version = ",
            )
            .bind(current.aggregate_version)
            .append(", last_observed_at = ")
            .bind(current.last_observed_at)
            .append(", timeout_deadline_at = null, updated_at = ")
            .bind(updated_at)
            .append(" where organization_id = ")
            .bind(current.organization_id.as_uuid())
            .append(" and node_id = ")
            .bind(current.id.as_uuid())
            .append(" and state = ")
            .bind(head.state.as_str())
            .append(" and last_observed_at = ")
            .bind(head.last_observed_at),
        )
        .await?,
    )
}

async fn resolve_head(
    transaction: &PostgresTransaction,
    current: &Node,
    head: &AvailabilityHeadRow,
    reason: NodeAvailabilityResolutionReason,
    event: &a3s_cloud_contracts::DomainEventEnvelope,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Node availability resolution",
        execute(
            transaction,
            sql_query::<()>(
                "update fleet_node_availability_fact_heads set state = 'resolved', node_aggregate_version = ",
            )
            .bind(current.aggregate_version)
            .append(", last_observed_at = ")
            .bind(current.last_observed_at)
            .append(", timeout_deadline_at = null, latest_event_id = ")
            .bind(event.event_id)
            .append(", latest_event_key = ")
            .bind(event.event_key.as_str())
            .append(", latest_phase_version = ")
            .bind(event.aggregate_version)
            .append(", resolved_at = ")
            .bind(event.occurred_at)
            .append(", resolution_reason = ")
            .bind(reason.as_str())
            .append(", updated_at = ")
            .bind(event.occurred_at)
            .append(" where organization_id = ")
            .bind(current.organization_id.as_uuid())
            .append(" and node_id = ")
            .bind(current.id.as_uuid())
            .append(" and state = 'unavailable' and firing_event_id = ")
            .bind(head.firing_event_id.ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "unavailable Node fact head omitted its firing event".into(),
                )
            })?),
        )
        .await?,
    )
}
