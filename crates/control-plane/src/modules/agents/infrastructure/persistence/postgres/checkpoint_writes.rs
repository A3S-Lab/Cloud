use super::checkpoint_queries::load_checkpoint;
use super::queries::{
    load_conversation_by_id, load_event_range, load_execution_by_id, lock_conversation,
    lock_execution,
};
use super::schema::AgentExecutionCheckpoints;
use super::writes::{insert_event, insert_execution, persist_conversation};
use crate::infrastructure::{
    execute, idempotency_replay, is_foreign_key_violation, is_unique_violation, require_one_row,
    store_idempotency, store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::agents::domain::{
    AgentConversationStatus, AgentExecution, AgentExecutionCheckpoint,
    AgentExecutionCheckpointWrite, AgentExecutionCheckpointWriteReference, AgentExecutionEvent,
    AgentExecutionTelemetryCorrelation, AgentExecutionWrite, AgentExecutionWriteReference,
    CommitAgentExecutionCheckpointWrite, ForkAgentExecutionWrite,
};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, RepositoryError};
use a3s_orm::{insert_into, PostgresExecutor, PostgresTransaction};

pub(super) async fn commit_checkpoint(
    executor: &PostgresExecutor,
    write: CommitAgentExecutionCheckpointWrite,
) -> Result<AgentExecutionCheckpointWrite, RepositoryError> {
    write.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replayed) =
                    replay_checkpoint_in_transaction(transaction, &write.idempotency).await?
                {
                    return Ok(AgentExecutionCheckpointWrite {
                        checkpoint: replayed,
                        replayed: true,
                    });
                }
                // The shared idempotency probe already serializes one key. Lock
                // the execution afterwards so captures with different keys can
                // deterministically adopt the same boundary descriptor instead
                // of racing into its unique constraint.
                lock_execution(
                    transaction,
                    write.checkpoint.organization_id,
                    write.checkpoint.execution_id,
                )
                .await?;
                if let Some(existing) = load_checkpoint(
                    transaction,
                    write.checkpoint.organization_id,
                    write.checkpoint.id,
                )
                .await?
                {
                    if existing != write.checkpoint {
                        return Err(RepositoryError::Conflict(
                            "Agent checkpoint identity is already bound to different content"
                                .into(),
                        )
                        .into());
                    }
                    store_checkpoint_replay(transaction, &write.idempotency, &existing).await?;
                    return Ok(AgentExecutionCheckpointWrite {
                        checkpoint: existing,
                        replayed: true,
                    });
                }

                validate_checkpoint_authority(transaction, &write.checkpoint).await?;
                insert_checkpoint(transaction, &write.checkpoint).await?;
                store_outbox(transaction, &write.event).await?;
                store_checkpoint_replay(transaction, &write.idempotency, &write.checkpoint).await?;
                Ok(AgentExecutionCheckpointWrite {
                    checkpoint: write.checkpoint,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn fork_execution(
    executor: &PostgresExecutor,
    write: ForkAgentExecutionWrite,
) -> Result<AgentExecutionWrite, RepositoryError> {
    write.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replayed) = replay_fork(transaction, &write.idempotency).await? {
                    return Ok(replayed);
                }
                let lineage = write.execution.lineage.as_ref().ok_or_else(|| {
                    RepositoryError::Conflict("Agent execution fork lineage is missing".into())
                })?;
                let mut conversation = lock_conversation(
                    transaction,
                    write.execution.organization_id,
                    write.execution.conversation_id,
                )
                .await?;
                if conversation.status != AgentConversationStatus::Active {
                    return Err(RepositoryError::Conflict(
                        "closed Agent conversation cannot fork an execution".into(),
                    )
                    .into());
                }
                let parent = load_execution_by_id(
                    transaction,
                    write.execution.organization_id,
                    lineage.parent_execution_id,
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                let checkpoint = load_checkpoint(
                    transaction,
                    write.execution.organization_id,
                    lineage.parent_checkpoint_id,
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                let expected = AgentExecution::fork_from(
                    &parent,
                    &checkpoint,
                    write.execution.id,
                    write.execution.operation_id,
                    write.execution.requested_at,
                )
                .map_err(RepositoryError::Conflict)?;
                if expected != write.execution
                    || checkpoint.execution_id != parent.id
                    || lineage.parent_checkpoint_digest != checkpoint.object.digest
                {
                    return Err(RepositoryError::Conflict(
                        "Agent execution fork changed its committed checkpoint lineage".into(),
                    )
                    .into());
                }

                let previous_conversation_version = conversation.aggregate_version;
                let first_sequence = conversation
                    .allocate_event_sequences(1, write.initial_event.occurred_at)
                    .map_err(RepositoryError::Conflict)?;
                let initial_event = AgentExecutionEvent::from_draft(
                    write.execution.organization_id,
                    write.execution.conversation_id,
                    write.execution.id,
                    first_sequence,
                    write.initial_event,
                )
                .map_err(invalid_repository_write)?;

                persist_conversation(transaction, &conversation, previous_conversation_version)
                    .await?;
                insert_execution(transaction, &write.execution).await?;
                insert_event(transaction, &initial_event).await?;
                store_outbox(transaction, &write.event).await?;
                store_idempotency(
                    transaction,
                    &write.idempotency,
                    &AgentExecutionWriteReference {
                        organization_id: write.execution.organization_id,
                        execution_id: write.execution.id,
                    },
                )
                .await?;
                Ok(AgentExecutionWrite {
                    conversation,
                    execution: write.execution,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

async fn validate_checkpoint_authority(
    transaction: &PostgresTransaction,
    checkpoint: &AgentExecutionCheckpoint,
) -> Result<(), PostgresPersistenceError> {
    let execution = load_execution_by_id(
        transaction,
        checkpoint.organization_id,
        checkpoint.execution_id,
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    let conversation = load_conversation_by_id(
        transaction,
        checkpoint.organization_id,
        checkpoint.conversation_id,
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    let boundary = load_event_range(
        transaction,
        checkpoint.organization_id,
        checkpoint.conversation_id,
        checkpoint.execution_id,
        checkpoint.through_event_sequence,
        checkpoint.through_event_sequence,
    )
    .await?;
    let boundary = boundary.first().ok_or(RepositoryError::NotFound)?;
    let telemetry = AgentExecutionTelemetryCorrelation::from_execution(&execution)
        .map_err(RepositoryError::Conflict)?;
    let invocation_digest = execution
        .code
        .as_ref()
        .ok_or(RepositoryError::NotFound)?
        .require_invocation_profile()
        .map_err(RepositoryError::Conflict)?
        .digest()
        .map_err(RepositoryError::Conflict)?;
    if execution.conversation_id != checkpoint.conversation_id
        || conversation.project_id != checkpoint.project_id
        || conversation.environment_id != checkpoint.environment_id
        || boundary.occurred_at != checkpoint.captured_at
        || execution.agent.artifact_digest() != &checkpoint.agent_artifact_digest
        || execution.provider.profile_digest() != checkpoint.provider_profile_digest.as_str()
        || invocation_digest != checkpoint.invocation_profile_digest.as_str()
        || telemetry != checkpoint.telemetry_correlation
    {
        return Err(RepositoryError::Conflict(
            "Agent checkpoint changed its execution or event authority".into(),
        )
        .into());
    }
    Ok(())
}

async fn insert_checkpoint(
    transaction: &PostgresTransaction,
    checkpoint: &AgentExecutionCheckpoint,
) -> Result<(), PostgresPersistenceError> {
    let telemetry = &checkpoint.telemetry_correlation;
    let object = &checkpoint.object;
    let inserted = execute(
        transaction,
        insert_into::<AgentExecutionCheckpoints>()
            .value(
                AgentExecutionCheckpoints::organization_id(),
                checkpoint.organization_id.as_uuid(),
            )
            .value(
                AgentExecutionCheckpoints::project_id(),
                checkpoint.project_id.as_uuid(),
            )
            .value(
                AgentExecutionCheckpoints::environment_id(),
                checkpoint.environment_id.as_uuid(),
            )
            .value(
                AgentExecutionCheckpoints::conversation_id(),
                checkpoint.conversation_id.as_uuid(),
            )
            .value(
                AgentExecutionCheckpoints::execution_id(),
                checkpoint.execution_id.as_uuid(),
            )
            .value(AgentExecutionCheckpoints::id(), checkpoint.id.as_uuid())
            .value(
                AgentExecutionCheckpoints::through_event_sequence(),
                checkpoint.through_event_sequence,
            )
            .value(
                AgentExecutionCheckpoints::event_count(),
                checkpoint.event_count,
            )
            .value(
                AgentExecutionCheckpoints::agent_artifact_digest(),
                checkpoint.agent_artifact_digest.as_str(),
            )
            .value(
                AgentExecutionCheckpoints::provider_profile_digest(),
                checkpoint.provider_profile_digest.as_str(),
            )
            .value(
                AgentExecutionCheckpoints::invocation_profile_digest(),
                checkpoint.invocation_profile_digest.as_str(),
            )
            .value(
                AgentExecutionCheckpoints::object_schema(),
                object.schema.as_str(),
            )
            .value(
                AgentExecutionCheckpoints::object_namespace(),
                object.namespace.as_str(),
            )
            .value(
                AgentExecutionCheckpoints::object_ref(),
                object.object_ref.as_str(),
            )
            .value(
                AgentExecutionCheckpoints::object_digest(),
                object.digest.as_str(),
            )
            .value(
                AgentExecutionCheckpoints::object_size_bytes(),
                object.size_bytes,
            )
            .value(
                AgentExecutionCheckpoints::object_media_type(),
                object.media_type.as_str(),
            )
            .value(
                AgentExecutionCheckpoints::operation_id(),
                telemetry.operation_id.as_uuid(),
            )
            .value(
                AgentExecutionCheckpoints::provider_run_identity_digest(),
                telemetry.provider_run_identity_digest.as_str(),
            )
            .value(
                AgentExecutionCheckpoints::node_id(),
                telemetry.node_id.as_uuid(),
            )
            .value(
                AgentExecutionCheckpoints::workload_id(),
                telemetry.workload_id.as_uuid(),
            )
            .value(
                AgentExecutionCheckpoints::workload_revision_id(),
                telemetry.workload_revision_id.as_uuid(),
            )
            .value(
                AgentExecutionCheckpoints::deployment_id(),
                telemetry.deployment_id.as_uuid(),
            )
            .value(
                AgentExecutionCheckpoints::replica_id(),
                telemetry.replica_id.as_uuid(),
            )
            .value(
                AgentExecutionCheckpoints::runtime_unit_id(),
                telemetry.runtime_unit_id.as_str(),
            )
            .value(
                AgentExecutionCheckpoints::runtime_generation(),
                telemetry.runtime_generation,
            )
            .value(
                AgentExecutionCheckpoints::aggregate_version(),
                checkpoint.aggregate_version,
            )
            .value(
                AgentExecutionCheckpoints::captured_at(),
                checkpoint.captured_at,
            ),
    )
    .await;
    match inserted {
        Ok(rows) => require_one_row("Agent execution checkpoint", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "Agent execution checkpoint identity is already in use".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

async fn replay_checkpoint_in_transaction(
    transaction: &PostgresTransaction,
    idempotency: &IdempotencyRequest,
) -> Result<Option<AgentExecutionCheckpoint>, PostgresPersistenceError> {
    let Some(replay) =
        idempotency_replay::<AgentExecutionCheckpointWriteReference>(transaction, idempotency)
            .await?
    else {
        return Ok(None);
    };
    load_checkpoint(
        transaction,
        replay.value.organization_id,
        replay.value.checkpoint_id,
    )
    .await?
    .map(Some)
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Agent checkpoint replay target is missing".into())
    })
}

pub(super) async fn replay_checkpoint(
    executor: &PostgresExecutor,
    idempotency: &IdempotencyRequest,
) -> Result<Option<AgentExecutionCheckpoint>, RepositoryError> {
    let idempotency = idempotency.clone();
    executor
        .transaction(move |transaction| {
            Box::pin(
                async move { replay_checkpoint_in_transaction(transaction, &idempotency).await },
            )
        })
        .await
        .map_err(transaction_error)
}

async fn store_checkpoint_replay(
    transaction: &PostgresTransaction,
    idempotency: &IdempotencyRequest,
    checkpoint: &AgentExecutionCheckpoint,
) -> Result<(), PostgresPersistenceError> {
    store_idempotency(
        transaction,
        idempotency,
        &AgentExecutionCheckpointWriteReference {
            organization_id: checkpoint.organization_id,
            checkpoint_id: checkpoint.id,
        },
    )
    .await
}

async fn replay_fork(
    transaction: &PostgresTransaction,
    idempotency: &IdempotencyRequest,
) -> Result<Option<AgentExecutionWrite>, PostgresPersistenceError> {
    let Some(replay) =
        idempotency_replay::<AgentExecutionWriteReference>(transaction, idempotency).await?
    else {
        return Ok(None);
    };
    let execution = load_execution_by_id(
        transaction,
        replay.value.organization_id,
        replay.value.execution_id,
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Agent fork replay target is missing".into())
    })?;
    let conversation = load_conversation_by_id(
        transaction,
        execution.organization_id,
        execution.conversation_id,
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Agent fork conversation is missing".into())
    })?;
    Ok(Some(AgentExecutionWrite {
        conversation,
        execution,
        replayed: true,
    }))
}

fn invalid_repository_write(message: impl Into<String>) -> RepositoryError {
    RepositoryError::Conflict(format!(
        "invalid Agent checkpoint repository write: {}",
        message.into()
    ))
}
