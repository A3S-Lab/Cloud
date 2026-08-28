use super::approval_queries::{load_checkpoint, lock_active_checkpoint, lock_checkpoint};
use super::queries::{lock_conversation, lock_execution};
use super::schema::AgentApprovalCheckpoints;
use super::writes::{insert_event, persist_conversation, persist_execution};
use crate::infrastructure::{
    execute, idempotency_replay, is_foreign_key_violation, is_unique_violation, require_one_row,
    store_audit, store_idempotency, transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::agents::domain::{
    AgentApprovalCheckpoint, AgentApprovalCheckpointStatus, AgentApprovalCheckpointWrite,
    AgentApprovalCheckpointWriteReference, AgentExecutionEvent, AgentExecutionStatus,
    CancelActiveAgentApprovalCheckpointWrite, DecideAgentApprovalCheckpointWrite,
    ExpireAgentApprovalCheckpointWrite, ResumeAgentApprovalCheckpointWrite,
};
use crate::modules::shared_kernel::domain::{RepositoryError, Sha256Digest};
use a3s_cloud_contracts::AgentProviderApprovalOutcomeV1;
use a3s_orm::{insert_into, update_table, PostgresExecutor, PostgresTransaction};

const AGENT_APPROVAL_AUDIT_SCHEMA_V1: &str = "a3s.cloud.agent-approval-audit.v1";

pub(super) async fn replay_checkpoint_decision(
    executor: &PostgresExecutor,
    idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
) -> Result<Option<AgentApprovalCheckpoint>, RepositoryError> {
    let idempotency = idempotency.clone();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let Some(replay) = idempotency_replay::<AgentApprovalCheckpointWriteReference>(
                    transaction,
                    &idempotency,
                )
                .await?
                else {
                    return Ok(None);
                };
                let checkpoint = load_checkpoint(
                    transaction,
                    replay.value.organization_id,
                    replay.value.checkpoint_id,
                )
                .await?
                .ok_or_else(|| invariant("Agent approval decision replay target is missing"))?;
                Ok(Some(checkpoint))
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn decide_checkpoint(
    executor: &PostgresExecutor,
    write: DecideAgentApprovalCheckpointWrite,
) -> Result<AgentApprovalCheckpointWrite, RepositoryError> {
    write.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) = replay_decision(transaction, &write).await? {
                    return Ok(replay);
                }
                let probe =
                    load_checkpoint(transaction, write.organization_id, write.checkpoint_id)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                let (mut conversation, mut execution, mut checkpoint) =
                    lock_resolution_context(transaction, &probe).await?;
                checkpoint
                    .decide(
                        write.expected_version,
                        write.decision_id,
                        write.outcome,
                        write.decided_by,
                        write.authorization_decision,
                        write.reason,
                        write.decided_at,
                    )
                    .map_err(RepositoryError::Conflict)?;
                persist_resolution(transaction, &mut conversation, &mut execution, &checkpoint)
                    .await?;
                store_approval_audit(
                    transaction,
                    &checkpoint,
                    decision_action(write.outcome),
                    checkpoint.decided_by.map(|value| value.as_uuid()),
                    write.request_id,
                    checkpoint
                        .decided_at
                        .ok_or_else(|| invariant("Agent approval decision time is missing"))?,
                )
                .await?;
                store_idempotency(
                    transaction,
                    &write.idempotency,
                    &AgentApprovalCheckpointWriteReference {
                        organization_id: checkpoint.organization_id,
                        checkpoint_id: checkpoint.id,
                    },
                )
                .await?;
                Ok(AgentApprovalCheckpointWrite {
                    checkpoint,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn expire_checkpoint(
    executor: &PostgresExecutor,
    write: ExpireAgentApprovalCheckpointWrite,
) -> Result<AgentApprovalCheckpointWrite, RepositoryError> {
    write.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let probe =
                    load_checkpoint(transaction, write.organization_id, write.checkpoint_id)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                let (mut conversation, mut execution, mut checkpoint) =
                    lock_resolution_context(transaction, &probe).await?;
                checkpoint
                    .expire(write.expected_version, write.decision_id, write.expired_at)
                    .map_err(RepositoryError::Conflict)?;
                persist_resolution(transaction, &mut conversation, &mut execution, &checkpoint)
                    .await?;
                store_approval_audit(
                    transaction,
                    &checkpoint,
                    "agent.execution.approval-expired",
                    None,
                    write.decision_id.as_uuid(),
                    write.expired_at,
                )
                .await?;
                Ok(AgentApprovalCheckpointWrite {
                    checkpoint,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn mark_checkpoint_resumed(
    executor: &PostgresExecutor,
    write: ResumeAgentApprovalCheckpointWrite,
) -> Result<AgentApprovalCheckpointWrite, RepositoryError> {
    write.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let probe =
                    load_checkpoint(transaction, write.organization_id, write.checkpoint_id)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                let (_, mut execution, mut checkpoint) =
                    lock_resolution_context(transaction, &probe).await?;
                let expected_digest = Sha256Digest::parse(
                    write
                        .command
                        .digest()
                        .map_err(PostgresPersistenceError::Invariant)?,
                )
                .map_err(PostgresPersistenceError::Invariant)?;
                if checkpoint.status == AgentApprovalCheckpointStatus::Resumed {
                    if checkpoint.resume_command_id == Some(write.command_id)
                        && checkpoint.resume_command_digest.as_ref() == Some(&expected_digest)
                    {
                        if execution.status != AgentExecutionStatus::Running {
                            return Err(invariant(
                                "resumed Agent approval checkpoint left its execution paused",
                            ));
                        }
                        return Ok(AgentApprovalCheckpointWrite {
                            checkpoint,
                            replayed: true,
                        });
                    }
                    return Err(RepositoryError::Conflict(
                        "Agent approval checkpoint resumed with another command".into(),
                    )
                    .into());
                }
                if execution.status != AgentExecutionStatus::AwaitingApproval {
                    return Err(RepositoryError::Conflict(
                        "Agent approval resume lost to execution cancellation or completion".into(),
                    )
                    .into());
                }
                let previous_checkpoint_version = checkpoint.aggregate_version;
                let previous_execution_version = execution.aggregate_version;
                let resumed_at = write
                    .resumed_at
                    .max(checkpoint.updated_at)
                    .max(execution.updated_at);
                checkpoint
                    .mark_resumed(
                        write.expected_version,
                        write.command_id,
                        &write.command,
                        resumed_at,
                    )
                    .map_err(RepositoryError::Conflict)?;
                execution
                    .resume_after_approval(resumed_at)
                    .map_err(RepositoryError::Conflict)?;
                persist_execution(transaction, &execution, previous_execution_version).await?;
                persist_checkpoint(transaction, &checkpoint, previous_checkpoint_version).await?;
                store_approval_audit(
                    transaction,
                    &checkpoint,
                    "agent.execution.approval-resumed",
                    None,
                    write.command_id.as_uuid(),
                    resumed_at,
                )
                .await?;
                Ok(AgentApprovalCheckpointWrite {
                    checkpoint,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn cancel_active_checkpoint(
    executor: &PostgresExecutor,
    write: CancelActiveAgentApprovalCheckpointWrite,
) -> Result<Option<AgentApprovalCheckpointWrite>, RepositoryError> {
    write.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let Some(mut checkpoint) =
                    lock_active_checkpoint(transaction, write.organization_id, write.execution_id)
                        .await?
                else {
                    return Ok(None);
                };
                let previous_version = checkpoint.aggregate_version;
                let cancelled_at = write.cancelled_at.max(checkpoint.updated_at);
                checkpoint
                    .cancel(previous_version, cancelled_at)
                    .map_err(RepositoryError::Conflict)?;
                persist_checkpoint(transaction, &checkpoint, previous_version).await?;
                store_approval_audit(
                    transaction,
                    &checkpoint,
                    "agent.execution.approval-cancelled",
                    None,
                    checkpoint.id.as_uuid(),
                    cancelled_at,
                )
                .await?;
                Ok(Some(AgentApprovalCheckpointWrite {
                    checkpoint,
                    replayed: false,
                }))
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn insert_checkpoint(
    transaction: &PostgresTransaction,
    checkpoint: &AgentApprovalCheckpoint,
) -> Result<(), PostgresPersistenceError> {
    checkpoint
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    let inserted = execute(
        transaction,
        insert_into::<AgentApprovalCheckpoints>()
            .value(
                AgentApprovalCheckpoints::organization_id(),
                checkpoint.organization_id.as_uuid(),
            )
            .value(
                AgentApprovalCheckpoints::project_id(),
                checkpoint.project_id.as_uuid(),
            )
            .value(
                AgentApprovalCheckpoints::environment_id(),
                checkpoint.environment_id.as_uuid(),
            )
            .value(
                AgentApprovalCheckpoints::conversation_id(),
                checkpoint.conversation_id.as_uuid(),
            )
            .value(
                AgentApprovalCheckpoints::execution_id(),
                checkpoint.execution_id.as_uuid(),
            )
            .value(AgentApprovalCheckpoints::id(), checkpoint.id.as_uuid())
            .value(
                AgentApprovalCheckpoints::provider_run_identity_digest(),
                checkpoint.provider_run_identity_digest.as_str(),
            )
            .value(
                AgentApprovalCheckpoints::invocation_profile_digest(),
                checkpoint.invocation_profile_digest.as_str(),
            )
            .value(
                AgentApprovalCheckpoints::source_event_sequence(),
                checkpoint.source_event_sequence,
            )
            .value(
                AgentApprovalCheckpoints::call_id(),
                checkpoint.call_id.as_str(),
            )
            .value(
                AgentApprovalCheckpoints::tool_name(),
                checkpoint.tool.name.as_str(),
            )
            .value(
                AgentApprovalCheckpoints::tool_revision(),
                checkpoint.tool.revision.as_str(),
            )
            .value(
                AgentApprovalCheckpoints::tool_contract_digest(),
                checkpoint.tool.contract_digest.as_str(),
            )
            .value(
                AgentApprovalCheckpoints::request_digest(),
                checkpoint.request.digest.as_str(),
            )
            .value(
                AgentApprovalCheckpoints::request_size_bytes(),
                checkpoint.request.size_bytes,
            )
            .value(
                AgentApprovalCheckpoints::request_media_type(),
                checkpoint.request.media_type.as_str(),
            )
            .value(
                AgentApprovalCheckpoints::status(),
                checkpoint.status.as_str(),
            )
            .value(
                AgentApprovalCheckpoints::aggregate_version(),
                checkpoint.aggregate_version,
            )
            .value(
                AgentApprovalCheckpoints::requested_at(),
                checkpoint.requested_at,
            )
            .value(
                AgentApprovalCheckpoints::expires_at(),
                checkpoint.expires_at,
            )
            .value(
                AgentApprovalCheckpoints::updated_at(),
                checkpoint.updated_at,
            ),
    )
    .await;
    match inserted {
        Ok(rows) => require_one_row("Agent approval checkpoint", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "Agent execution already has this or another active approval checkpoint".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

async fn lock_resolution_context(
    transaction: &PostgresTransaction,
    probe: &AgentApprovalCheckpoint,
) -> Result<
    (
        crate::modules::agents::domain::AgentConversation,
        crate::modules::agents::domain::AgentExecution,
        AgentApprovalCheckpoint,
    ),
    PostgresPersistenceError,
> {
    let conversation =
        lock_conversation(transaction, probe.organization_id, probe.conversation_id).await?;
    let execution = lock_execution(transaction, probe.organization_id, probe.execution_id).await?;
    let checkpoint = lock_checkpoint(transaction, probe.organization_id, probe.id).await?;
    if checkpoint.organization_id != probe.organization_id
        || checkpoint.project_id != probe.project_id
        || checkpoint.environment_id != probe.environment_id
        || checkpoint.conversation_id != probe.conversation_id
        || checkpoint.execution_id != probe.execution_id
        || checkpoint.id != probe.id
        || checkpoint.provider_run_identity_digest != probe.provider_run_identity_digest
        || checkpoint.invocation_profile_digest != probe.invocation_profile_digest
        || checkpoint.source_event_sequence != probe.source_event_sequence
        || checkpoint.call_id != probe.call_id
        || checkpoint.tool != probe.tool
        || checkpoint.request != probe.request
        || checkpoint.requested_at != probe.requested_at
        || checkpoint.expires_at != probe.expires_at
        || conversation.organization_id != checkpoint.organization_id
        || conversation.project_id != checkpoint.project_id
        || conversation.environment_id != checkpoint.environment_id
        || conversation.id != checkpoint.conversation_id
        || execution.organization_id != checkpoint.organization_id
        || execution.id != checkpoint.execution_id
        || execution.conversation_id != conversation.id
    {
        return Err(invariant(
            "Agent approval checkpoint changed immutable identity while acquiring transaction locks",
        ));
    }
    Ok((conversation, execution, checkpoint))
}

async fn persist_resolution(
    transaction: &PostgresTransaction,
    conversation: &mut crate::modules::agents::domain::AgentConversation,
    execution: &mut crate::modules::agents::domain::AgentExecution,
    checkpoint: &AgentApprovalCheckpoint,
) -> Result<(), PostgresPersistenceError> {
    let previous_conversation_version = conversation.aggregate_version;
    let previous_execution_version = execution.aggregate_version;
    let previous_checkpoint_version = checkpoint
        .aggregate_version
        .checked_sub(1)
        .ok_or_else(|| invariant("Agent approval checkpoint version underflowed"))?;
    let draft = checkpoint
        .resolution_event_draft()
        .map_err(PostgresPersistenceError::Invariant)?;
    execution
        .apply_event(&draft)
        .map_err(RepositoryError::Conflict)?;
    let sequence = conversation
        .allocate_event_sequences(1, draft.occurred_at)
        .map_err(RepositoryError::Conflict)?;
    let event = AgentExecutionEvent::from_draft(
        checkpoint.organization_id,
        checkpoint.conversation_id,
        checkpoint.execution_id,
        sequence,
        draft,
    )
    .map_err(PostgresPersistenceError::Invariant)?;
    persist_conversation(transaction, conversation, previous_conversation_version).await?;
    persist_execution(transaction, execution, previous_execution_version).await?;
    persist_checkpoint(transaction, checkpoint, previous_checkpoint_version).await?;
    insert_event(transaction, &event).await
}

async fn persist_checkpoint(
    transaction: &PostgresTransaction,
    checkpoint: &AgentApprovalCheckpoint,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    checkpoint
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    let rows = execute(
        transaction,
        update_table::<AgentApprovalCheckpoints>()
            .set(
                AgentApprovalCheckpoints::status(),
                checkpoint.status.as_str(),
            )
            .set(
                AgentApprovalCheckpoints::decision_id(),
                checkpoint.decision_id.map(|value| value.as_uuid()),
            )
            .set(
                AgentApprovalCheckpoints::outcome(),
                checkpoint.outcome.map(|value| value.as_str().to_owned()),
            )
            .set(
                AgentApprovalCheckpoints::decided_by(),
                checkpoint.decided_by.map(|value| value.as_uuid()),
            )
            .set(
                AgentApprovalCheckpoints::authorization_decision_id(),
                checkpoint
                    .authorization_decision
                    .as_ref()
                    .map(|value| value.id.clone()),
            )
            .set(
                AgentApprovalCheckpoints::authorization_decision_digest(),
                checkpoint
                    .authorization_decision
                    .as_ref()
                    .map(|value| value.digest.as_str().to_owned()),
            )
            .set(
                AgentApprovalCheckpoints::reason(),
                checkpoint.reason.clone(),
            )
            .set(
                AgentApprovalCheckpoints::decision_digest(),
                checkpoint
                    .decision_digest
                    .as_ref()
                    .map(|value| value.as_str().to_owned()),
            )
            .set(
                AgentApprovalCheckpoints::resume_command_id(),
                checkpoint.resume_command_id.map(|value| value.as_uuid()),
            )
            .set(
                AgentApprovalCheckpoints::resume_command_digest(),
                checkpoint
                    .resume_command_digest
                    .as_ref()
                    .map(|value| value.as_str().to_owned()),
            )
            .set(
                AgentApprovalCheckpoints::aggregate_version(),
                checkpoint.aggregate_version,
            )
            .set(
                AgentApprovalCheckpoints::updated_at(),
                checkpoint.updated_at,
            )
            .set(
                AgentApprovalCheckpoints::decided_at(),
                checkpoint.decided_at,
            )
            .set(
                AgentApprovalCheckpoints::resumed_at(),
                checkpoint.resumed_at,
            )
            .set(
                AgentApprovalCheckpoints::cancelled_at(),
                checkpoint.cancelled_at,
            )
            .filter(
                AgentApprovalCheckpoints::organization_id()
                    .eq(checkpoint.organization_id.as_uuid()),
            )
            .filter(AgentApprovalCheckpoints::id().eq(checkpoint.id.as_uuid()))
            .filter(AgentApprovalCheckpoints::aggregate_version().eq(expected_version)),
    )
    .await?;
    require_one_row("Agent approval checkpoint transition", rows)
}

async fn replay_decision(
    transaction: &PostgresTransaction,
    write: &DecideAgentApprovalCheckpointWrite,
) -> Result<Option<AgentApprovalCheckpointWrite>, PostgresPersistenceError> {
    let Some(replay) = idempotency_replay::<AgentApprovalCheckpointWriteReference>(
        transaction,
        &write.idempotency,
    )
    .await?
    else {
        return Ok(None);
    };
    if replay.value.organization_id != write.organization_id
        || replay.value.checkpoint_id != write.checkpoint_id
    {
        return Err(invariant(
            "Agent approval decision replay changed its immutable identity",
        ));
    }
    let checkpoint = load_checkpoint(
        transaction,
        replay.value.organization_id,
        replay.value.checkpoint_id,
    )
    .await?
    .ok_or_else(|| invariant("Agent approval decision replay target is missing"))?;
    Ok(Some(AgentApprovalCheckpointWrite {
        checkpoint,
        replayed: true,
    }))
}

async fn store_approval_audit(
    transaction: &PostgresTransaction,
    checkpoint: &AgentApprovalCheckpoint,
    action: &'static str,
    actor_id: Option<uuid::Uuid>,
    request_id: uuid::Uuid,
    occurred_at: chrono::DateTime<chrono::Utc>,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: uuid::Uuid::now_v7(),
            actor_id,
            action,
            aggregate_id: checkpoint.execution_id.as_uuid(),
            occurred_at,
            request_id,
            scope: AuditWrite::resource_scope(
                checkpoint.organization_id.as_uuid(),
                checkpoint.project_id,
                Some(checkpoint.environment_id),
            ),
            details: serde_json::json!({
                "schema": AGENT_APPROVAL_AUDIT_SCHEMA_V1,
                "projectId": checkpoint.project_id,
                "environmentId": checkpoint.environment_id,
                "conversationId": checkpoint.conversation_id,
                "executionId": checkpoint.execution_id,
                "checkpointId": checkpoint.id,
                "checkpointVersion": checkpoint.aggregate_version,
                "providerRunIdentityDigest": checkpoint.provider_run_identity_digest,
                "invocationProfileDigest": checkpoint.invocation_profile_digest,
                "providerSourceSequence": checkpoint.source_event_sequence,
                "callId": checkpoint.call_id,
                "tool": checkpoint.tool,
                "request": checkpoint.request,
                "outcome": checkpoint.outcome,
                "decisionId": checkpoint.decision_id,
                "decisionDigest": checkpoint.decision_digest,
                "authorizationDecision": checkpoint.authorization_decision,
                "resumeCommandId": checkpoint.resume_command_id,
                "resumeCommandDigest": checkpoint.resume_command_digest,
            }),
        },
    )
    .await
}

const fn decision_action(outcome: AgentProviderApprovalOutcomeV1) -> &'static str {
    match outcome {
        AgentProviderApprovalOutcomeV1::Approved => "agent.execution.approval-approved",
        AgentProviderApprovalOutcomeV1::Denied => "agent.execution.approval-denied",
        AgentProviderApprovalOutcomeV1::Expired => "agent.execution.approval-expired",
    }
}

fn invalid_repository_write(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!("invalid Agent approval repository write: {error}"))
}

fn invariant(message: impl Into<String>) -> PostgresPersistenceError {
    PostgresPersistenceError::Invariant(message.into())
}
