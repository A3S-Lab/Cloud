mod observations;
mod telemetry;

pub(super) use observations::{latest_runtime_observation, record_observations};
pub(super) use telemetry::{
    list_log_chunks, list_log_chunks_for_retention, mark_log_chunk_retained,
    record_gateway_acknowledgement, record_log_chunks, replay_log_batch,
};

use super::{nodes, queries};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, require_one_row, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::fleet::domain::entities::{NodeCommand, NodeCommandDraft};
use crate::modules::fleet::domain::repositories::{
    NodeHeartbeatUpdate, NodeLogBatchReceiptDraft, NodeLogBatchReplay, NodeLogChunkMetadata,
    NodeLogChunkQuery, NodeLogChunkReceiptDraft, NodeLogRetentionTarget, RuntimeObservationRecord,
};
use crate::modules::fleet::domain::value_objects::{NodeCapabilities, NodeState};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, IdempotentWrite, NodeCommandId, NodeId, RepositoryError,
};
use a3s_cloud_contracts::{
    GatewayAckState, NodeCommandAck, NodeCommandLeaseRequest, NodeCommandLeaseResponse,
    NodeCommandOutcome, NodeCommandPayload, NodeGatewayAck, NodeGatewayAckReceipt,
    NodeLogChunkReceipt, NodeObservationBatch, NodeObservationReceipt,
};
use a3s_orm::{sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, Row};
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

const SELECT_COMMANDS: &str = "select id, node_id, sequence, aggregate_id, generation, command_kind, payload_schema, payload_digest, payload, issued_at, not_after, correlation_id, lease_id, leased_to_agent_instance_id, leased_until, acknowledgement, completed_at from node_commands";

struct CommandRow {
    id: Uuid,
    node_id: Uuid,
    sequence: u64,
    aggregate_id: Uuid,
    generation: u64,
    command_kind: String,
    payload_schema: String,
    payload_digest: String,
    payload: Value,
    issued_at: DateTime<Utc>,
    not_after: DateTime<Utc>,
    correlation_id: Uuid,
    lease_id: Option<Uuid>,
    agent_instance_id: Option<Uuid>,
    leased_until: Option<DateTime<Utc>>,
    acknowledgement: Option<Value>,
    completed_at: Option<DateTime<Utc>>,
}

impl FromRow for CommandRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            node_id: decode(row, 1)?,
            sequence: decode(row, 2)?,
            aggregate_id: decode(row, 3)?,
            generation: decode(row, 4)?,
            command_kind: decode(row, 5)?,
            payload_schema: decode(row, 6)?,
            payload_digest: decode(row, 7)?,
            payload: decode(row, 8)?,
            issued_at: decode(row, 9)?,
            not_after: decode(row, 10)?,
            correlation_id: decode(row, 11)?,
            lease_id: decode(row, 12)?,
            agent_instance_id: decode(row, 13)?,
            leased_until: decode(row, 14)?,
            acknowledgement: decode(row, 15)?,
            completed_at: decode(row, 16)?,
        })
    }
}

impl CommandRow {
    fn command(&self) -> Result<NodeCommand, PostgresPersistenceError> {
        let payload: NodeCommandPayload = serde_json::from_value(self.payload.clone())?;
        let command = NodeCommand::issue(
            NodeCommandDraft {
                proposed_command_id: NodeCommandId::from_uuid(self.id),
                node_id: NodeId::from_uuid(self.node_id),
                aggregate_id: self.aggregate_id,
                payload,
                issued_at: self.issued_at,
                not_after: self.not_after,
                correlation_id: self.correlation_id,
            },
            self.sequence,
        )
        .map_err(PostgresPersistenceError::Invariant)?;
        if command.generation() != self.generation
            || command.kind() != self.command_kind
            || command.payload_schema() != self.payload_schema
            || command
                .payload_digest()
                .map_err(PostgresPersistenceError::Invariant)?
                != self.payload_digest
        {
            return Err(PostgresPersistenceError::Invariant(
                "stored node command metadata does not match its payload".into(),
            ));
        }
        Ok(command)
    }

    fn acknowledgement(&self) -> Result<Option<NodeCommandAck>, PostgresPersistenceError> {
        self.acknowledgement
            .clone()
            .map(serde_json::from_value)
            .transpose()
            .map_err(Into::into)
    }

    fn lease(&self) -> Result<Option<StoredLease>, PostgresPersistenceError> {
        match (self.lease_id, self.agent_instance_id, self.leased_until) {
            (None, None, None) => Ok(None),
            (Some(lease_id), Some(agent_instance_id), Some(leased_until)) => {
                Ok(Some(StoredLease {
                    lease_id,
                    agent_instance_id,
                    leased_until,
                }))
            }
            _ => Err(PostgresPersistenceError::Invariant(
                "stored node command has a partial lease".into(),
            )),
        }
    }

    fn validate_completion(&self) -> Result<(), PostgresPersistenceError> {
        if self.acknowledgement.is_some() != self.completed_at.is_some() {
            return Err(PostgresPersistenceError::Invariant(
                "stored node command has a partial acknowledgement".into(),
            ));
        }
        Ok(())
    }
}

struct StoredLease {
    lease_id: Uuid,
    agent_instance_id: Uuid,
    leased_until: DateTime<Utc>,
}

pub(super) async fn enqueue(
    executor: &PostgresExecutor,
    draft: NodeCommandDraft,
) -> Result<IdempotentWrite<NodeCommand>, RepositoryError> {
    NodeCommand::issue(draft.clone(), 1).map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let command_lock = draft.proposed_command_id.to_string();
                let locked = fetch_optional::<i32, _>(
                    transaction,
                    sql_query::<i32>(
                        "select 1 from (select pg_advisory_xact_lock(hashtext(",
                    )
                    .bind(command_lock.as_str())
                    .append("))) as locked"),
                )
                .await?;
                if locked != Some(1) {
                    return Err(PostgresPersistenceError::Invariant(
                        "node command advisory lock did not return a row".into(),
                    ));
                }
                if let Some(existing) = command_by_id(
                    transaction,
                    draft.proposed_command_id.as_uuid(),
                    true,
                )
                .await?
                {
                    let command = existing.command()?;
                    let retry = NodeCommand::issue(draft, command.sequence)
                        .map_err(RepositoryError::Conflict)?;
                    if retry != command {
                        return Err(RepositoryError::Conflict(
                            "node command ID was reused with different input".into(),
                        )
                        .into());
                    }
                    return Ok(IdempotentWrite {
                        value: command,
                        replayed: true,
                    });
                }

                let node = fetch_optional::<(String, u64), _>(
                    transaction,
                    sql_query::<(String, u64)>(
                        "select state, last_sequence from nodes where id = ",
                    )
                    .bind(draft.node_id.as_uuid())
                    .append(" for update"),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                if node.0 == "revoked" {
                    return Err(RepositoryError::NotFound.into());
                }

                if let NodeCommandPayload::RuntimeApply { request } = &draft.payload {
                    let requested_spec_digest = request
                        .spec
                        .digest()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    if let Some(existing) = fetch_optional::<CommandRow, _>(
                        transaction,
                        sql_query::<CommandRow>(SELECT_COMMANDS)
                            .append(" where node_id = ")
                            .bind(draft.node_id.as_uuid())
                            .append(" and aggregate_id = ")
                            .bind(draft.aggregate_id)
                            .append(" and command_kind = 'runtime_apply' and generation = ")
                            .bind(draft.payload.generation())
                            .append(" order by sequence desc limit 1 for update"),
                    )
                    .await?
                    {
                        let command = existing.command()?;
                        let NodeCommandPayload::RuntimeApply {
                            request: existing_request,
                        } = &command.payload
                        else {
                            return Err(PostgresPersistenceError::Invariant(
                                "stored Runtime apply command has the wrong payload kind".into(),
                            ));
                        };
                        if existing_request
                            .spec
                            .digest()
                            .map_err(PostgresPersistenceError::Invariant)?
                            != requested_spec_digest
                        {
                            return Err(RepositoryError::Conflict(
                                "Runtime apply generation was reused with a different specification"
                                    .into(),
                            )
                            .into());
                        }
                    }
                    let latest = fetch_optional::<Option<u64>, _>(
                        transaction,
                        sql_query::<Option<u64>>(
                            "select max(generation) from node_commands where node_id = ",
                        )
                        .bind(draft.node_id.as_uuid())
                        .append(" and aggregate_id = ")
                        .bind(draft.aggregate_id)
                        .append(" and command_kind = 'runtime_apply'"),
                    )
                    .await?
                    .flatten();
                    if latest.is_some_and(|generation| generation > draft.payload.generation()) {
                        return Err(RepositoryError::Conflict(
                            "Runtime apply generation regressed".into(),
                        )
                        .into());
                    }
                }

                let sequence = node.1.checked_add(1).ok_or_else(|| {
                    RepositoryError::Conflict("node command sequence exhausted".into())
                })?;
                let command = NodeCommand::issue(draft, sequence)
                    .map_err(RepositoryError::Conflict)?;
                require_one_row(
                    "node command sequence",
                    execute(
                        transaction,
                        sql_query::<()>("update nodes set last_sequence = ")
                            .bind(sequence)
                            .append(" where id = ")
                            .bind(command.node_id.as_uuid())
                            .append(" and last_sequence = ")
                            .bind(node.1),
                    )
                    .await?,
                )?;
                require_one_row(
                    "node command",
                    execute(
                        transaction,
                        sql_query::<()>(
                            "insert into node_commands (id, node_id, sequence, aggregate_id, generation, command_kind, payload_schema, payload_digest, payload, issued_at, not_after, correlation_id) values (",
                        )
                        .bind(command.id.as_uuid())
                        .append(", ")
                        .bind(command.node_id.as_uuid())
                        .append(", ")
                        .bind(command.sequence)
                        .append(", ")
                        .bind(command.aggregate_id)
                        .append(", ")
                        .bind(command.generation())
                        .append(", ")
                        .bind(command.kind())
                        .append(", ")
                        .bind(command.payload_schema())
                        .append(", ")
                        .bind(
                            command
                                .payload_digest()
                                .map_err(PostgresPersistenceError::Invariant)?,
                        )
                        .append(", ")
                        .bind(serde_json::to_value(&command.payload)?)
                        .append(", ")
                        .bind(command.issued_at)
                        .append(", ")
                        .bind(command.not_after)
                        .append(", ")
                        .bind(command.correlation_id)
                        .append(")"),
                    )
                    .await?,
                )?;
                Ok(IdempotentWrite {
                    value: command,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn find_command(
    executor: &PostgresExecutor,
    node_id: NodeId,
    command_id: NodeCommandId,
) -> Result<Option<NodeCommand>, RepositoryError> {
    a3s_orm::Database::new(a3s_orm::PostgresDialect, executor.clone())
        .fetch_optional_as(
            sql_query::<CommandRow>(SELECT_COMMANDS)
                .append(" where node_id = ")
                .bind(node_id.as_uuid())
                .append(" and id = ")
                .bind(command_id.as_uuid()),
        )
        .await
        .map_err(|error| RepositoryError::Storage(error.to_string()))?
        .map(|row| {
            row.command()
                .map_err(|error| RepositoryError::Storage(error.to_string()))
        })
        .transpose()
}

pub(super) async fn lease(
    executor: &PostgresExecutor,
    request: &NodeCommandLeaseRequest,
    lease_id: Uuid,
    now: DateTime<Utc>,
    leased_until: DateTime<Utc>,
) -> Result<NodeCommandLeaseResponse, RepositoryError> {
    let now = canonical_timestamp(now);
    let leased_until = canonical_timestamp(leased_until);
    request.validate().map_err(RepositoryError::Conflict)?;
    if lease_id.is_nil() || leased_until <= now {
        return Err(RepositoryError::Conflict(
            "command lease identity or expiry is invalid".into(),
        ));
    }
    let request = request.clone();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let node = fetch_optional::<(String, Uuid), _>(
                    transaction,
                    sql_query::<(String, Uuid)>(
                        "select state, agent_instance_id from nodes where id = ",
                    )
                    .bind(request.node_id)
                    .append(" for update"),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                if node.0 == "revoked" || node.1 != request.agent_instance_id {
                    return Err(RepositoryError::NotFound.into());
                }

                let candidates = fetch_all::<CommandRow, _>(
                    transaction,
                    sql_query::<CommandRow>(SELECT_COMMANDS)
                        .append(" where node_id = ")
                        .bind(request.node_id)
                        .append(" and sequence > ")
                        .bind(request.after_sequence)
                        .append(" and acknowledgement is null")
                        .append(" order by sequence limit ")
                        .bind(i64::from(request.max_commands))
                        .append(" for update"),
                )
                .await?;
                let mut commands = Vec::with_capacity(candidates.len());
                for row in candidates {
                    row.validate_completion()?;
                    if row
                        .lease()?
                        .is_some_and(|existing| existing.leased_until > now)
                    {
                        break;
                    }
                    let command = row.command()?;
                    require_one_row(
                        "node command lease",
                        execute(
                            transaction,
                            sql_query::<()>("update node_commands set lease_id = ")
                                .bind(lease_id)
                                .append(", leased_to_agent_instance_id = ")
                                .bind(request.agent_instance_id)
                                .append(", leased_until = ")
                                .bind(leased_until)
                                .append(" where id = ")
                                .bind(command.id.as_uuid())
                                .append(" and acknowledgement is null"),
                        )
                        .await?,
                    )?;
                    commands.push(
                        command
                            .envelope(lease_id)
                            .map_err(PostgresPersistenceError::Invariant)?,
                    );
                }
                let response = NodeCommandLeaseResponse {
                    schema: NodeCommandLeaseResponse::SCHEMA.into(),
                    lease_id,
                    node_id: request.node_id,
                    agent_instance_id: request.agent_instance_id,
                    leased_until,
                    commands,
                };
                response
                    .validate(now)
                    .map_err(PostgresPersistenceError::Invariant)?;
                Ok(response)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn acknowledge(
    executor: &PostgresExecutor,
    mut acknowledgement: NodeCommandAck,
    _received_at: DateTime<Utc>,
) -> Result<IdempotentWrite<NodeCommandAck>, RepositoryError> {
    acknowledgement.completed_at = canonical_timestamp(acknowledgement.completed_at);
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let row = command_by_id(transaction, acknowledgement.command_id, true)
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                row.validate_completion()?;
                if let Some(existing) = row.acknowledgement()? {
                    if existing != acknowledgement {
                        return Err(RepositoryError::Conflict(
                            "command acknowledgement was replayed with different content".into(),
                        )
                        .into());
                    }
                    return Ok(IdempotentWrite {
                        value: existing,
                        replayed: true,
                    });
                }
                let lease = row.lease()?.ok_or_else(|| {
                    RepositoryError::Conflict("command has not been leased".into())
                })?;
                if lease.agent_instance_id.is_nil() {
                    return Err(PostgresPersistenceError::Invariant(
                        "stored command lease has no agent identity".into(),
                    ));
                }
                let command = row.command()?;
                let envelope = command
                    .envelope(lease.lease_id)
                    .map_err(PostgresPersistenceError::Invariant)?;
                acknowledgement
                    .validate_against(&envelope)
                    .map_err(RepositoryError::Conflict)?;
                if acknowledgement.completed_at > lease.leased_until {
                    return Err(RepositoryError::Conflict(
                        "command acknowledgement completed after its lease expired".into(),
                    )
                    .into());
                }
                if matches!(
                    acknowledgement.outcome,
                    NodeCommandOutcome::Succeeded { .. }
                ) && acknowledgement.completed_at > command.not_after
                {
                    return Err(RepositoryError::Conflict(
                        "successful command acknowledgement completed after command expiry".into(),
                    )
                    .into());
                }
                require_one_row(
                    "node command acknowledgement",
                    execute(
                        transaction,
                        sql_query::<()>("update node_commands set acknowledgement = ")
                            .bind(serde_json::to_value(&acknowledgement)?)
                            .append(", completed_at = ")
                            .bind(acknowledgement.completed_at)
                            .append(" where id = ")
                            .bind(command.id.as_uuid())
                            .append(" and acknowledgement is null"),
                    )
                    .await?,
                )?;
                Ok(IdempotentWrite {
                    value: acknowledgement,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn command_acknowledgement(
    executor: &PostgresExecutor,
    node_id: NodeId,
    command_id: NodeCommandId,
) -> Result<Option<NodeCommandAck>, RepositoryError> {
    let row = a3s_orm::Database::new(a3s_orm::PostgresDialect, executor.clone())
        .fetch_optional_as(
            sql_query::<CommandRow>(SELECT_COMMANDS)
                .append(" where node_id = ")
                .bind(node_id.as_uuid())
                .append(" and id = ")
                .bind(command_id.as_uuid()),
        )
        .await
        .map_err(|error| RepositoryError::Storage(error.to_string()))?;
    let Some(row) = row else {
        return Ok(None);
    };
    row.validate_completion()
        .map_err(|error| RepositoryError::Storage(error.to_string()))?;
    row.acknowledgement()
        .map_err(|error| RepositoryError::Storage(error.to_string()))
}

async fn command_by_id(
    transaction: &a3s_orm::PostgresTransaction,
    command_id: Uuid,
    lock: bool,
) -> Result<Option<CommandRow>, PostgresPersistenceError> {
    let mut query = sql_query::<CommandRow>(SELECT_COMMANDS)
        .append(" where id = ")
        .bind(command_id);
    if lock {
        query = query.append(" for update");
    }
    fetch_optional(transaction, query).await
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}
