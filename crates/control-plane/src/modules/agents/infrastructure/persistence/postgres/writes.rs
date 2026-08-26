use super::queries::{
    load_conversation_by_id, load_event_range, load_execution_by_id, lock_conversation,
    lock_execution,
};
use super::schema::{
    AgentConversations, AgentExecutionChangeSets, AgentExecutionEvents, AgentExecutions,
};
use crate::infrastructure::{
    execute, idempotency_replay, is_foreign_key_violation, is_unique_violation, require_one_row,
    store_idempotency, store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::agents::domain::{
    AgentCodeRunWrite, AgentConversationStatus, AgentConversationWrite,
    AgentConversationWriteReference, AgentExecutionChangeSet, AgentExecutionEvent,
    AgentExecutionEventDraft, AgentExecutionEventsWrite, AgentExecutionEventsWriteReference,
    AgentExecutionWrite, AgentExecutionWriteReference, AppendAgentExecutionEventsWrite,
    BindAgentCodeRunWrite, CreateAgentConversationWrite, RequestAgentExecutionCancellationWrite,
    StartAgentExecutionWrite,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_orm::{insert_into, update_table, PostgresExecutor, PostgresTransaction};

pub(super) async fn create_conversation(
    executor: &PostgresExecutor,
    write: CreateAgentConversationWrite,
) -> Result<AgentConversationWrite, RepositoryError> {
    write.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) = replay_conversation(transaction, &write).await? {
                    return Ok(replay);
                }
                let conversation = &write.conversation;
                let inserted = execute(
                    transaction,
                    insert_into::<AgentConversations>()
                        .value(
                            AgentConversations::organization_id(),
                            conversation.organization_id.as_uuid(),
                        )
                        .value(
                            AgentConversations::project_id(),
                            conversation.project_id.as_uuid(),
                        )
                        .value(
                            AgentConversations::environment_id(),
                            conversation.environment_id.as_uuid(),
                        )
                        .value(AgentConversations::id(), conversation.id.as_uuid())
                        .value(AgentConversations::status(), conversation.status.as_str())
                        .value(
                            AgentConversations::last_event_sequence(),
                            conversation.last_event_sequence,
                        )
                        .value(
                            AgentConversations::aggregate_version(),
                            conversation.aggregate_version,
                        )
                        .value(AgentConversations::created_at(), conversation.created_at)
                        .value(AgentConversations::updated_at(), conversation.updated_at)
                        .value(AgentConversations::closed_at(), conversation.closed_at),
                )
                .await;
                match inserted {
                    Ok(rows) => require_one_row("Agent conversation", rows)?,
                    Err(error) if is_unique_violation(&error) => {
                        return Err(RepositoryError::Conflict(
                            "Agent conversation identity is already in use".into(),
                        )
                        .into())
                    }
                    Err(error) if is_foreign_key_violation(&error) => {
                        return Err(RepositoryError::NotFound.into())
                    }
                    Err(error) => return Err(error),
                }
                store_outbox(transaction, &write.event).await?;
                store_idempotency(
                    transaction,
                    &write.idempotency,
                    &AgentConversationWriteReference {
                        organization_id: conversation.organization_id,
                        conversation_id: conversation.id,
                    },
                )
                .await?;
                Ok(AgentConversationWrite {
                    conversation: write.conversation,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn start_execution(
    executor: &PostgresExecutor,
    write: StartAgentExecutionWrite,
) -> Result<AgentExecutionWrite, RepositoryError> {
    write.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) = replay_execution(transaction, &write).await? {
                    return Ok(replay);
                }
                let mut conversation = lock_conversation(
                    transaction,
                    write.execution.organization_id,
                    write.execution.conversation_id,
                )
                .await?;
                if conversation.status != AgentConversationStatus::Active {
                    return Err(RepositoryError::Conflict(
                        "closed Agent conversation cannot start an execution".into(),
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

pub(super) async fn request_cancellation(
    executor: &PostgresExecutor,
    write: RequestAgentExecutionCancellationWrite,
) -> Result<AgentExecutionWrite, RepositoryError> {
    write.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) = replay_cancellation(transaction, &write).await? {
                    return Ok(replay);
                }
                let existing = lock_execution(
                    transaction,
                    write.execution.organization_id,
                    write.execution.id,
                )
                .await?;
                let mut expected = existing.clone();
                if existing.aggregate_version != write.expected_version
                    || expected
                        .request_cancellation(write.execution.updated_at)
                        .is_err()
                    || expected != write.execution
                {
                    return Err(RepositoryError::Conflict(
                        "Agent execution changed while requesting cancellation".into(),
                    )
                    .into());
                }
                let conversation = load_conversation_by_id(
                    transaction,
                    write.execution.organization_id,
                    write.execution.conversation_id,
                )
                .await?
                .ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "Agent execution conversation is missing".into(),
                    )
                })?;
                persist_execution(transaction, &write.execution, write.expected_version).await?;
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

pub(super) async fn append_events(
    executor: &PostgresExecutor,
    write: AppendAgentExecutionEventsWrite,
) -> Result<AgentExecutionEventsWrite, RepositoryError> {
    write.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) = replay_events(transaction, &write).await? {
                    return Ok(replay);
                }
                let mut conversation =
                    lock_conversation(transaction, write.organization_id, write.conversation_id)
                        .await?;
                let previous_conversation_version = conversation.aggregate_version;
                let mut execution =
                    lock_execution(transaction, write.organization_id, write.execution_id).await?;
                if execution.conversation_id != write.conversation_id {
                    return Err(RepositoryError::NotFound.into());
                }
                let previous_execution_version = execution.aggregate_version;
                for event in &write.events {
                    execution
                        .apply_event(event)
                        .map_err(RepositoryError::Conflict)?;
                }
                let last_occurred_at = write
                    .events
                    .last()
                    .expect("validated non-empty Agent event batch")
                    .occurred_at;
                let first_sequence = conversation
                    .allocate_event_sequences(write.events.len(), last_occurred_at)
                    .map_err(RepositoryError::Conflict)?;
                let events = materialize_events(&write, first_sequence)?;
                let last_sequence = events
                    .last()
                    .expect("validated non-empty Agent event batch")
                    .sequence;

                persist_conversation(transaction, &conversation, previous_conversation_version)
                    .await?;
                persist_execution(transaction, &execution, previous_execution_version).await?;
                for event in &events {
                    insert_event(transaction, event).await?;
                }
                store_idempotency(
                    transaction,
                    &write.idempotency,
                    &AgentExecutionEventsWriteReference {
                        organization_id: write.organization_id,
                        conversation_id: write.conversation_id,
                        execution_id: write.execution_id,
                        first_sequence,
                        last_sequence,
                    },
                )
                .await?;
                Ok(AgentExecutionEventsWrite {
                    conversation,
                    execution,
                    events,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn bind_code_run(
    executor: &PostgresExecutor,
    write: BindAgentCodeRunWrite,
) -> Result<AgentCodeRunWrite, RepositoryError> {
    write.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let mut execution =
                    lock_execution(transaction, write.organization_id, write.execution_id).await?;
                let previous_version = execution.aggregate_version;
                let changed = execution
                    .bind_code_run(write.binding)
                    .map_err(RepositoryError::Conflict)?;
                if changed {
                    persist_execution(transaction, &execution, previous_version).await?;
                }
                Ok(AgentCodeRunWrite {
                    execution,
                    replayed: !changed,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

fn materialize_events(
    write: &AppendAgentExecutionEventsWrite,
    first_sequence: u64,
) -> Result<Vec<AgentExecutionEvent>, PostgresPersistenceError> {
    materialize_event_drafts(
        write.organization_id,
        write.conversation_id,
        write.execution_id,
        write.events.clone(),
        first_sequence,
    )
}

pub(super) fn materialize_event_drafts(
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    conversation_id: crate::modules::shared_kernel::domain::AgentConversationId,
    execution_id: crate::modules::shared_kernel::domain::AgentExecutionId,
    drafts: Vec<AgentExecutionEventDraft>,
    first_sequence: u64,
) -> Result<Vec<AgentExecutionEvent>, PostgresPersistenceError> {
    drafts
        .into_iter()
        .enumerate()
        .map(|(offset, draft)| {
            let offset = u64::try_from(offset).map_err(|_| {
                PostgresPersistenceError::Invariant("Agent event sequence offset overflowed".into())
            })?;
            let sequence = first_sequence.checked_add(offset).ok_or_else(|| {
                PostgresPersistenceError::Invariant("Agent event sequence overflowed".into())
            })?;
            AgentExecutionEvent::from_draft(
                organization_id,
                conversation_id,
                execution_id,
                sequence,
                draft,
            )
            .map_err(invalid_repository_write)
            .map_err(PostgresPersistenceError::from)
        })
        .collect()
}

async fn insert_execution(
    transaction: &PostgresTransaction,
    execution: &crate::modules::agents::domain::AgentExecution,
) -> Result<(), PostgresPersistenceError> {
    let inserted = execute(
        transaction,
        insert_into::<AgentExecutions>()
            .value(
                AgentExecutions::organization_id(),
                execution.organization_id.as_uuid(),
            )
            .value(
                AgentExecutions::conversation_id(),
                execution.conversation_id.as_uuid(),
            )
            .value(AgentExecutions::id(), execution.id.as_uuid())
            .value(
                AgentExecutions::operation_id(),
                execution.operation_id.as_uuid(),
            )
            .value(
                AgentExecutions::agent_asset_id(),
                execution.agent.asset_id().as_uuid(),
            )
            .value(
                AgentExecutions::agent_asset_release_id(),
                execution.agent.asset_release_id().as_uuid(),
            )
            .value(
                AgentExecutions::agent_build_run_id(),
                execution.agent.build_run_id().as_uuid(),
            )
            .value(
                AgentExecutions::agent_artifact_uri(),
                execution.agent.artifact_uri(),
            )
            .value(
                AgentExecutions::agent_artifact_digest(),
                execution.agent.artifact_digest().as_str(),
            )
            .value(
                AgentExecutions::agent_artifact_media_type(),
                execution.agent.artifact_media_type(),
            )
            .value(
                AgentExecutions::agent_artifact_size_bytes(),
                execution.agent.artifact_size_bytes(),
            )
            .value(AgentExecutions::status(), execution.status.as_str())
            .value(AgentExecutions::failure(), execution.failure.clone())
            .value(
                AgentExecutions::aggregate_version(),
                execution.aggregate_version,
            )
            .value(AgentExecutions::requested_at(), execution.requested_at)
            .value(AgentExecutions::updated_at(), execution.updated_at)
            .value(AgentExecutions::started_at(), execution.started_at)
            .value(
                AgentExecutions::cancellation_requested_at(),
                execution.cancellation_requested_at,
            )
            .value(AgentExecutions::finished_at(), execution.finished_at)
            .value(
                AgentExecutions::provider_kind(),
                Some(execution.provider.kind().to_owned()),
            )
            .value(
                AgentExecutions::provider_revision(),
                Some(execution.provider.revision().to_owned()),
            )
            .value(
                AgentExecutions::provider_protocol(),
                Some(execution.provider.protocol().to_owned()),
            )
            .value(
                AgentExecutions::provider_native_protocol(),
                Some(execution.provider.native_protocol().to_owned()),
            )
            .value(
                AgentExecutions::provider_profile_acl(),
                Some(execution.provider.profile_acl().to_owned()),
            )
            .value(
                AgentExecutions::provider_profile_digest(),
                Some(execution.provider.profile_digest().to_owned()),
            )
            .value(
                AgentExecutions::provider_capability_digest(),
                Some(execution.provider.capability_digest().to_owned()),
            ),
    )
    .await;
    match inserted {
        Ok(rows) => require_one_row("Agent execution", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "Agent execution or Operation identity is already in use".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

pub(super) async fn insert_event(
    transaction: &PostgresTransaction,
    event: &AgentExecutionEvent,
) -> Result<(), PostgresPersistenceError> {
    let inserted = execute(
        transaction,
        insert_into::<AgentExecutionEvents>()
            .value(
                AgentExecutionEvents::organization_id(),
                event.organization_id.as_uuid(),
            )
            .value(
                AgentExecutionEvents::conversation_id(),
                event.conversation_id.as_uuid(),
            )
            .value(AgentExecutionEvents::sequence(), event.sequence)
            .value(
                AgentExecutionEvents::execution_id(),
                event.execution_id.as_uuid(),
            )
            .value(AgentExecutionEvents::kind(), event.kind.as_str())
            .value(
                AgentExecutionEvents::content(),
                event.content.value().clone(),
            )
            .value(
                AgentExecutionEvents::content_digest(),
                event.content.digest().as_str(),
            )
            .value(
                AgentExecutionEvents::content_size_bytes(),
                event.content.size_bytes(),
            )
            .value(AgentExecutionEvents::occurred_at(), event.occurred_at),
    )
    .await;
    match inserted {
        Ok(rows) => require_one_row("Agent execution event", rows),
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Invariant(
            "Agent event sequence is already committed".into(),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

pub(super) async fn insert_change_set(
    transaction: &PostgresTransaction,
    change_set: &AgentExecutionChangeSet,
) -> Result<(), PostgresPersistenceError> {
    let encoded = serde_json::to_value(&change_set.change_set).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "Agent execution change set could not be encoded: {error}"
        ))
    })?;
    let inserted = execute(
        transaction,
        insert_into::<AgentExecutionChangeSets>()
            .value(
                AgentExecutionChangeSets::organization_id(),
                change_set.organization_id.as_uuid(),
            )
            .value(
                AgentExecutionChangeSets::execution_id(),
                change_set.execution_id.as_uuid(),
            )
            .value(AgentExecutionChangeSets::batch_id(), change_set.batch_id)
            .value(
                AgentExecutionChangeSets::node_id(),
                change_set.node_id.as_uuid(),
            )
            .value(AgentExecutionChangeSets::change_set(), encoded)
            .value(
                AgentExecutionChangeSets::recorded_at(),
                change_set.recorded_at,
            ),
    )
    .await;
    match inserted {
        Ok(rows) => require_one_row("Agent execution change set", rows),
        Err(error) if is_unique_violation(&error) => {
            Err(RepositoryError::Conflict("Agent execution change set is immutable".into()).into())
        }
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

pub(super) async fn persist_conversation(
    transaction: &PostgresTransaction,
    conversation: &crate::modules::agents::domain::AgentConversation,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        update_table::<AgentConversations>()
            .set(AgentConversations::status(), conversation.status.as_str())
            .set(
                AgentConversations::last_event_sequence(),
                conversation.last_event_sequence,
            )
            .set(
                AgentConversations::aggregate_version(),
                conversation.aggregate_version,
            )
            .set(AgentConversations::updated_at(), conversation.updated_at)
            .set(AgentConversations::closed_at(), conversation.closed_at)
            .filter(
                AgentConversations::organization_id().eq(conversation.organization_id.as_uuid()),
            )
            .filter(AgentConversations::id().eq(conversation.id.as_uuid()))
            .filter(AgentConversations::aggregate_version().eq(expected_version)),
    )
    .await?;
    require_one_row("Agent conversation transition", rows)
}

pub(super) async fn persist_execution(
    transaction: &PostgresTransaction,
    execution: &crate::modules::agents::domain::AgentExecution,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let code = execution.code.as_ref();
    execution.provider.validate().map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "Agent provider profile binding is invalid: {error}"
        ))
    })?;
    let provider = &execution.provider;
    let rows = execute(
        transaction,
        update_table::<AgentExecutions>()
            .set(AgentExecutions::status(), execution.status.as_str())
            .set(AgentExecutions::failure(), execution.failure.clone())
            .set(
                AgentExecutions::aggregate_version(),
                execution.aggregate_version,
            )
            .set(AgentExecutions::updated_at(), execution.updated_at)
            .set(AgentExecutions::started_at(), execution.started_at)
            .set(
                AgentExecutions::cancellation_requested_at(),
                execution.cancellation_requested_at,
            )
            .set(AgentExecutions::finished_at(), execution.finished_at)
            .set(
                AgentExecutions::provider_kind(),
                Some(provider.kind().to_owned()),
            )
            .set(
                AgentExecutions::provider_revision(),
                Some(provider.revision().to_owned()),
            )
            .set(
                AgentExecutions::provider_protocol(),
                Some(provider.protocol().to_owned()),
            )
            .set(
                AgentExecutions::provider_native_protocol(),
                Some(provider.native_protocol().to_owned()),
            )
            .set(
                AgentExecutions::provider_profile_acl(),
                Some(provider.profile_acl().to_owned()),
            )
            .set(
                AgentExecutions::provider_profile_digest(),
                Some(provider.profile_digest().to_owned()),
            )
            .set(
                AgentExecutions::provider_capability_digest(),
                Some(provider.capability_digest().to_owned()),
            )
            .set(
                AgentExecutions::provider_node_id(),
                code.map(|binding| binding.node_id().as_uuid()),
            )
            .set(
                AgentExecutions::provider_workload_id(),
                code.map(|binding| binding.workload_id().as_uuid()),
            )
            .set(
                AgentExecutions::provider_workload_revision_id(),
                code.map(|binding| binding.workload_revision_id().as_uuid()),
            )
            .set(
                AgentExecutions::provider_deployment_id(),
                code.map(|binding| binding.deployment_id().as_uuid()),
            )
            .set(
                AgentExecutions::provider_replica_id(),
                code.map(|binding| binding.replica_id().as_uuid()),
            )
            .set(
                AgentExecutions::provider_runtime_unit_id(),
                code.map(|binding| binding.runtime_unit_id().to_owned()),
            )
            .set(
                AgentExecutions::provider_runtime_generation(),
                code.map(|binding| binding.runtime_generation()),
            )
            .set(
                AgentExecutions::provider_runtime_spec_digest(),
                code.map(|binding| binding.runtime_spec_digest().as_str().to_owned()),
            )
            .set(
                AgentExecutions::provider_service_port_name(),
                code.map(|binding| binding.service_port_name().to_owned()),
            )
            .set(
                AgentExecutions::provider_release_identity(),
                code.map(|binding| binding.identity().agent_release_identity.clone()),
            )
            .set(
                AgentExecutions::provider_session_id(),
                code.map(|binding| binding.identity().session_id.clone()),
            )
            .set(
                AgentExecutions::provider_run_id(),
                code.map(|binding| binding.identity().run_id.clone()),
            )
            .set(
                AgentExecutions::provider_event_cursor(),
                code.and_then(|binding| binding.accepted_after_event_sequence()),
            )
            .set(
                AgentExecutions::provider_state(),
                code.map(|binding| binding.observed_state().as_str().to_owned()),
            )
            .set(
                AgentExecutions::provider_bound_at(),
                code.map(|binding| binding.bound_at()),
            )
            .set(
                AgentExecutions::provider_observed_at(),
                code.and_then(|binding| binding.observed_at()),
            )
            .filter(AgentExecutions::organization_id().eq(execution.organization_id.as_uuid()))
            .filter(AgentExecutions::id().eq(execution.id.as_uuid()))
            .filter(AgentExecutions::aggregate_version().eq(expected_version)),
    )
    .await;
    match rows {
        Ok(rows) => require_one_row("Agent execution transition", rows),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

async fn replay_conversation(
    transaction: &PostgresTransaction,
    write: &CreateAgentConversationWrite,
) -> Result<Option<AgentConversationWrite>, PostgresPersistenceError> {
    let Some(replay) =
        idempotency_replay::<AgentConversationWriteReference>(transaction, &write.idempotency)
            .await?
    else {
        return Ok(None);
    };
    if replay.value.organization_id != write.conversation.organization_id {
        return Err(PostgresPersistenceError::Invariant(
            "Agent conversation replay changed tenant".into(),
        ));
    }
    let conversation = load_conversation_by_id(
        transaction,
        replay.value.organization_id,
        replay.value.conversation_id,
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Agent conversation replay target is missing".into())
    })?;
    Ok(Some(AgentConversationWrite {
        conversation,
        replayed: true,
    }))
}

async fn replay_execution(
    transaction: &PostgresTransaction,
    write: &StartAgentExecutionWrite,
) -> Result<Option<AgentExecutionWrite>, PostgresPersistenceError> {
    let Some(replay) =
        idempotency_replay::<AgentExecutionWriteReference>(transaction, &write.idempotency).await?
    else {
        return Ok(None);
    };
    if replay.value.organization_id != write.execution.organization_id {
        return Err(PostgresPersistenceError::Invariant(
            "Agent execution replay changed tenant".into(),
        ));
    }
    let execution = load_execution_by_id(
        transaction,
        replay.value.organization_id,
        replay.value.execution_id,
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Agent execution replay target is missing".into())
    })?;
    let conversation = load_conversation_by_id(
        transaction,
        execution.organization_id,
        execution.conversation_id,
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Agent execution conversation is missing".into())
    })?;
    Ok(Some(AgentExecutionWrite {
        conversation,
        execution,
        replayed: true,
    }))
}

async fn replay_cancellation(
    transaction: &PostgresTransaction,
    write: &RequestAgentExecutionCancellationWrite,
) -> Result<Option<AgentExecutionWrite>, PostgresPersistenceError> {
    let Some(replay) =
        idempotency_replay::<AgentExecutionWriteReference>(transaction, &write.idempotency).await?
    else {
        return Ok(None);
    };
    if replay.value.organization_id != write.execution.organization_id
        || replay.value.execution_id != write.execution.id
    {
        return Err(PostgresPersistenceError::Invariant(
            "Agent cancellation replay changed its immutable identity".into(),
        ));
    }
    let execution = load_execution_by_id(
        transaction,
        replay.value.organization_id,
        replay.value.execution_id,
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Agent cancellation replay target is missing".into())
    })?;
    let conversation = load_conversation_by_id(
        transaction,
        execution.organization_id,
        execution.conversation_id,
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Agent execution conversation is missing".into())
    })?;
    Ok(Some(AgentExecutionWrite {
        conversation,
        execution,
        replayed: true,
    }))
}

async fn replay_events(
    transaction: &PostgresTransaction,
    write: &AppendAgentExecutionEventsWrite,
) -> Result<Option<AgentExecutionEventsWrite>, PostgresPersistenceError> {
    let Some(replay) =
        idempotency_replay::<AgentExecutionEventsWriteReference>(transaction, &write.idempotency)
            .await?
    else {
        return Ok(None);
    };
    let reference = replay.value;
    if reference.organization_id != write.organization_id
        || reference.conversation_id != write.conversation_id
        || reference.execution_id != write.execution_id
        || reference.first_sequence == 0
        || reference.last_sequence < reference.first_sequence
    {
        return Err(PostgresPersistenceError::Invariant(
            "Agent event replay changed its immutable range".into(),
        ));
    }
    let conversation = load_conversation_by_id(
        transaction,
        reference.organization_id,
        reference.conversation_id,
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Agent event replay conversation is missing".into())
    })?;
    let execution = load_execution_by_id(
        transaction,
        reference.organization_id,
        reference.execution_id,
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Agent event replay execution is missing".into())
    })?;
    let events = load_event_range(
        transaction,
        reference.organization_id,
        reference.conversation_id,
        reference.execution_id,
        reference.first_sequence,
        reference.last_sequence,
    )
    .await?;
    let expected_count = reference
        .last_sequence
        .checked_sub(reference.first_sequence)
        .and_then(|count| count.checked_add(1))
        .and_then(|count| usize::try_from(count).ok())
        .ok_or_else(|| {
            PostgresPersistenceError::Invariant("Agent event replay range overflowed".into())
        })?;
    let exact = events.len() == expected_count
        && events.iter().enumerate().all(|(offset, event)| {
            u64::try_from(offset)
                .ok()
                .and_then(|offset| reference.first_sequence.checked_add(offset))
                == Some(event.sequence)
        });
    if !exact {
        return Err(PostgresPersistenceError::Invariant(
            "Agent event replay range is incomplete".into(),
        ));
    }
    Ok(Some(AgentExecutionEventsWrite {
        conversation,
        execution,
        events,
        replayed: true,
    }))
}

fn invalid_repository_write(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!("invalid Agent repository write: {error}"))
}
