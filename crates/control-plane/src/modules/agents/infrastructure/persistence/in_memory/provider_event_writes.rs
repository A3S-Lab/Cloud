use super::{
    corrupt, invalid_repository_write, replay, store_replay, IdempotencyResponse,
    InMemoryAgentRepository,
};
use crate::modules::agents::domain::{
    project_agent_approval_checkpoint, AcceptAgentProviderEventBatchWrite,
    AgentApprovalCheckpointStatus, AgentExecutionEvent, AgentExecutionEventDraft,
    AgentExecutionStatus,
};
use crate::modules::shared_kernel::domain::{AgentExecutionId, RepositoryError};
use a3s_cloud_contracts::{AgentProviderSemanticEventV1, NodeAgentProviderEventReceiptV1};

pub(super) async fn accept_provider_event_batch(
    repository: &InMemoryAgentRepository,
    write: AcceptAgentProviderEventBatchWrite,
) -> Result<NodeAgentProviderEventReceiptV1, RepositoryError> {
    write.validate().map_err(invalid_repository_write)?;
    let mut state = repository.state.write().await;
    if let Some(response) = replay(&state, &write.idempotency)? {
        let IdempotencyResponse::ProviderEvents(mut receipt) = response else {
            return Err(corrupt("Agent provider event replay type changed"));
        };
        receipt.receipt.replayed = true;
        receipt.validate_for(&write.batch).map_err(|error| {
            corrupt(format!(
                "Agent provider event replay changed its immutable receipt: {error}"
            ))
        })?;
        return Ok(receipt);
    }

    let execution_id = AgentExecutionId::from_uuid(write.batch.binding.execution_id);
    let execution_key = (write.organization_id, execution_id);
    let mut execution = state
        .executions
        .get(&execution_key)
        .cloned()
        .ok_or(RepositoryError::NotFound)?;
    let conversation_key = (write.organization_id, execution.conversation_id);
    let mut conversation = state
        .conversations
        .get(&conversation_key)
        .cloned()
        .ok_or_else(|| corrupt("Agent execution conversation is missing"))?;
    let binding = execution
        .code
        .as_ref()
        .cloned()
        .ok_or(RepositoryError::NotFound)?;
    if binding.node_id() != write.authenticated_node_id {
        return Err(RepositoryError::NotFound);
    }
    let current_binding = binding
        .node_provider_runtime_binding(execution.id.as_uuid())
        .map_err(corrupt)?;
    if current_binding != write.batch.binding {
        if binding
            .can_settle_recovery_predecessor_provider_runtime_binding(
                &write.batch.binding,
                execution.id,
            )
            .map_err(corrupt)?
        {
            let receipt = write.receipt(false).map_err(corrupt)?;
            store_replay(
                &mut state,
                write.idempotency,
                IdempotencyResponse::ProviderEvents(receipt.clone()),
            );
            return Ok(receipt);
        }
        return Err(RepositoryError::Conflict(
            "Agent provider event batch changed its bound Runtime or run identity".into(),
        ));
    }

    let active_checkpoint = state
        .checkpoints
        .values()
        .filter(|checkpoint| {
            checkpoint.organization_id == execution.organization_id
                && checkpoint.execution_id == execution.id
        })
        .find(|checkpoint| !checkpoint.status.is_terminal())
        .cloned();
    let latest_checkpoint = state
        .checkpoints
        .values()
        .filter(|checkpoint| {
            checkpoint.organization_id == execution.organization_id
                && checkpoint.execution_id == execution.id
        })
        .max_by(|left, right| {
            left.requested_at
                .cmp(&right.requested_at)
                .then_with(|| left.id.cmp(&right.id))
        })
        .cloned();
    let page_identity_digest = write.batch.page.identity.digest().map_err(corrupt)?;
    if execution.status == AgentExecutionStatus::AwaitingApproval
        && active_checkpoint.is_none()
        && !latest_checkpoint.as_ref().is_some_and(|checkpoint| {
            checkpoint.status == AgentApprovalCheckpointStatus::Resumed
                && checkpoint.provider_run_identity_digest.as_str() == page_identity_digest.as_str()
                && checkpoint.invocation_profile_digest.as_str()
                    == write
                        .batch
                        .page
                        .identity
                        .invocation_profile_digest
                        .as_deref()
                        .unwrap_or_default()
                && write.batch.page.after_event_sequence == Some(checkpoint.source_event_sequence)
        })
    {
        return Err(RepositoryError::Conflict(
            "awaiting Agent provider advanced without an exact approval resume".into(),
        ));
    }
    if active_checkpoint.is_some()
        && (write.batch.page.retention_gap
            || write.batch.page.state
                != a3s_cloud_contracts::AgentProviderRunStateV1::AwaitingApproval
            || write.batch.page.events.iter().any(|record| {
                matches!(
                    &record.event,
                    AgentProviderSemanticEventV1::ToolRequest { .. }
                        | AgentProviderSemanticEventV1::ToolResult { .. }
                )
            }))
    {
        return Err(RepositoryError::Conflict(
            "paused Agent provider advanced before exact resume".into(),
        ));
    }

    let projected_at = write.accepted_at.max(execution.updated_at);
    let drafts = if write.batch.page.retention_gap {
        binding
            .validate_provider_recovery_page(&write.batch.page)
            .map_err(RepositoryError::Conflict)?;
        execution
            .recover_code_run(&binding, projected_at)
            .map_err(RepositoryError::Conflict)?;
        Vec::new()
    } else {
        let drafts =
            AgentExecutionEventDraft::semantic_from_provider_page(&write.batch.page, projected_at)
                .map_err(invalid_repository_write)?;
        execution
            .accept_provider_event_page(&write.batch.page, projected_at, &drafts)
            .map_err(RepositoryError::Conflict)?;
        drafts
    };
    let checkpoint = if write.batch.page.retention_gap {
        None
    } else {
        project_agent_approval_checkpoint(
            &conversation,
            &execution,
            &write.batch.page,
            projected_at,
        )
        .map_err(invalid_repository_write)?
    };
    if execution.status == AgentExecutionStatus::AwaitingApproval
        && active_checkpoint.is_none()
        && checkpoint.is_none()
    {
        return Err(RepositoryError::Conflict(
            "Agent provider entered approval without an exact checkpoint".into(),
        ));
    }

    let events = if drafts.is_empty() {
        Vec::new()
    } else {
        let last_occurred_at = drafts
            .last()
            .ok_or_else(|| corrupt("non-empty Agent provider event page omitted its last draft"))?
            .occurred_at;
        let first_sequence = conversation
            .allocate_event_sequences(drafts.len(), last_occurred_at)
            .map_err(RepositoryError::Conflict)?;
        drafts
            .into_iter()
            .enumerate()
            .map(|(offset, draft)| {
                let offset = u64::try_from(offset)
                    .map_err(|_| corrupt("Agent event sequence offset overflowed"))?;
                let sequence = first_sequence
                    .checked_add(offset)
                    .ok_or_else(|| corrupt("Agent event sequence overflowed"))?;
                AgentExecutionEvent::from_draft(
                    write.organization_id,
                    conversation.id,
                    execution.id,
                    sequence,
                    draft,
                )
                .map_err(invalid_repository_write)
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    for event in &events {
        let key = (event.organization_id, event.conversation_id, event.sequence);
        if state.events.contains_key(&key) {
            return Err(corrupt("Agent event sequence is already committed"));
        }
    }
    let checkpoint_key = checkpoint.as_ref().map(|checkpoint| {
        let key = (checkpoint.organization_id, checkpoint.id);
        let has_conflict = state.checkpoints.contains_key(&key)
            || state.checkpoints.values().any(|existing| {
                existing.organization_id == checkpoint.organization_id
                    && existing.execution_id == checkpoint.execution_id
                    && !existing.status.is_terminal()
            });
        (key, has_conflict)
    });
    if checkpoint_key
        .as_ref()
        .is_some_and(|(_, has_conflict)| *has_conflict)
    {
        return Err(RepositoryError::Conflict(
            "Agent execution already has an active approval checkpoint".into(),
        ));
    }

    let receipt = write.receipt(false).map_err(corrupt)?;
    if !events.is_empty() {
        state.conversations.insert(conversation_key, conversation);
    }
    state.executions.insert(execution_key, execution);
    if let (Some(checkpoint), Some((key, _))) = (checkpoint, checkpoint_key) {
        state.checkpoints.insert(key, checkpoint);
    }
    for event in events {
        state.events.insert(
            (event.organization_id, event.conversation_id, event.sequence),
            event,
        );
    }
    store_replay(
        &mut state,
        write.idempotency,
        IdempotencyResponse::ProviderEvents(receipt.clone()),
    );
    Ok(receipt)
}
