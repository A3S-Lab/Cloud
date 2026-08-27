use super::{
    corrupt, invalid_repository_write, replay, store_replay, IdempotencyResponse,
    InMemoryAgentRepository, State,
};
use crate::modules::agents::domain::{
    AgentApprovalCheckpoint, AgentApprovalCheckpointStatus, AgentApprovalCheckpointWrite,
    AgentExecutionEvent, AgentExecutionStatus, CancelActiveAgentApprovalCheckpointWrite,
    DecideAgentApprovalCheckpointWrite, ExpireAgentApprovalCheckpointWrite,
    IAgentApprovalCheckpointRepository, ResumeAgentApprovalCheckpointWrite,
};
use crate::modules::shared_kernel::domain::{
    AgentApprovalCheckpointId, AgentExecutionId, OrganizationId, RepositoryError, Sha256Digest,
};
use async_trait::async_trait;

#[async_trait]
impl IAgentApprovalCheckpointRepository for InMemoryAgentRepository {
    async fn replay_checkpoint_decision(
        &self,
        idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
    ) -> Result<Option<AgentApprovalCheckpoint>, RepositoryError> {
        let state = self.state.read().await;
        let Some(response) = replay(&state, idempotency)? else {
            return Ok(None);
        };
        let IdempotencyResponse::Approval(organization_id, checkpoint_id) = response else {
            return Err(corrupt("Agent approval replay type changed"));
        };
        state
            .checkpoints
            .get(&(organization_id, checkpoint_id))
            .cloned()
            .map(Some)
            .ok_or_else(|| corrupt("Agent approval replay target is missing"))
    }

    async fn decide_checkpoint(
        &self,
        write: DecideAgentApprovalCheckpointWrite,
    ) -> Result<AgentApprovalCheckpointWrite, RepositoryError> {
        write.validate().map_err(invalid_repository_write)?;
        let mut state = self.state.write().await;
        if let Some(response) = replay(&state, &write.idempotency)? {
            let IdempotencyResponse::Approval(organization_id, checkpoint_id) = response else {
                return Err(corrupt("Agent approval replay type changed"));
            };
            let checkpoint = state
                .checkpoints
                .get(&(organization_id, checkpoint_id))
                .cloned()
                .ok_or_else(|| corrupt("Agent approval replay target is missing"))?;
            return Ok(AgentApprovalCheckpointWrite {
                checkpoint,
                replayed: true,
            });
        }

        let key = (write.organization_id, write.checkpoint_id);
        let mut checkpoint = state
            .checkpoints
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
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
        append_resolution(&mut state, &checkpoint)?;
        state.checkpoints.insert(key, checkpoint.clone());
        store_replay(
            &mut state,
            write.idempotency,
            IdempotencyResponse::Approval(write.organization_id, write.checkpoint_id),
        );
        Ok(AgentApprovalCheckpointWrite {
            checkpoint,
            replayed: false,
        })
    }

    async fn expire_checkpoint(
        &self,
        write: ExpireAgentApprovalCheckpointWrite,
    ) -> Result<AgentApprovalCheckpointWrite, RepositoryError> {
        write.validate().map_err(invalid_repository_write)?;
        let mut state = self.state.write().await;
        let key = (write.organization_id, write.checkpoint_id);
        let mut checkpoint = state
            .checkpoints
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        checkpoint
            .expire(write.expected_version, write.decision_id, write.expired_at)
            .map_err(RepositoryError::Conflict)?;
        append_resolution(&mut state, &checkpoint)?;
        state.checkpoints.insert(key, checkpoint.clone());
        Ok(AgentApprovalCheckpointWrite {
            checkpoint,
            replayed: false,
        })
    }

    async fn mark_checkpoint_resumed(
        &self,
        write: ResumeAgentApprovalCheckpointWrite,
    ) -> Result<AgentApprovalCheckpointWrite, RepositoryError> {
        write.validate().map_err(invalid_repository_write)?;
        let mut state = self.state.write().await;
        let key = (write.organization_id, write.checkpoint_id);
        let mut checkpoint = state
            .checkpoints
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let expected_digest =
            Sha256Digest::parse(write.command.digest().map_err(corrupt)?).map_err(corrupt)?;
        if checkpoint.status == AgentApprovalCheckpointStatus::Resumed {
            if checkpoint.resume_command_id == Some(write.command_id)
                && checkpoint.resume_command_digest.as_ref() == Some(&expected_digest)
            {
                let execution = state
                    .executions
                    .get(&(checkpoint.organization_id, checkpoint.execution_id))
                    .ok_or_else(|| corrupt("Agent approval execution is missing"))?;
                if execution.status != AgentExecutionStatus::Running {
                    return Err(corrupt(
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
            ));
        }
        let mut execution = state
            .executions
            .get(&(checkpoint.organization_id, checkpoint.execution_id))
            .cloned()
            .ok_or_else(|| corrupt("Agent approval execution is missing"))?;
        if execution.status != AgentExecutionStatus::AwaitingApproval {
            return Err(RepositoryError::Conflict(
                "Agent approval resume lost to execution cancellation or completion".into(),
            ));
        }
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
        state.executions.insert(
            (checkpoint.organization_id, checkpoint.execution_id),
            execution,
        );
        state.checkpoints.insert(key, checkpoint.clone());
        Ok(AgentApprovalCheckpointWrite {
            checkpoint,
            replayed: false,
        })
    }

    async fn cancel_active_checkpoint(
        &self,
        write: CancelActiveAgentApprovalCheckpointWrite,
    ) -> Result<Option<AgentApprovalCheckpointWrite>, RepositoryError> {
        write.validate().map_err(invalid_repository_write)?;
        let mut state = self.state.write().await;
        let key = state
            .checkpoints
            .iter()
            .find(|((organization_id, _), checkpoint)| {
                *organization_id == write.organization_id
                    && checkpoint.execution_id == write.execution_id
                    && !checkpoint.status.is_terminal()
            })
            .map(|(key, _)| *key);
        let Some(key) = key else {
            return Ok(None);
        };
        let mut checkpoint = state
            .checkpoints
            .get(&key)
            .cloned()
            .ok_or_else(|| corrupt("active Agent approval checkpoint disappeared"))?;
        let cancelled_at = write.cancelled_at.max(checkpoint.updated_at);
        checkpoint
            .cancel(checkpoint.aggregate_version, cancelled_at)
            .map_err(RepositoryError::Conflict)?;
        state.checkpoints.insert(key, checkpoint.clone());
        Ok(Some(AgentApprovalCheckpointWrite {
            checkpoint,
            replayed: false,
        }))
    }

    async fn find_checkpoint(
        &self,
        organization_id: OrganizationId,
        checkpoint_id: AgentApprovalCheckpointId,
    ) -> Result<Option<AgentApprovalCheckpoint>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .checkpoints
            .get(&(organization_id, checkpoint_id))
            .cloned())
    }

    async fn find_active_checkpoint(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
    ) -> Result<Option<AgentApprovalCheckpoint>, RepositoryError> {
        let values = self
            .state
            .read()
            .await
            .checkpoints
            .values()
            .filter(|checkpoint| {
                checkpoint.organization_id == organization_id
                    && checkpoint.execution_id == execution_id
                    && !checkpoint.status.is_terminal()
            })
            .cloned()
            .collect::<Vec<_>>();
        match values.as_slice() {
            [] => Ok(None),
            [checkpoint] => Ok(Some(checkpoint.clone())),
            _ => Err(corrupt(
                "Agent execution has multiple active approval checkpoints",
            )),
        }
    }

    async fn list_checkpoints(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
        status: Option<AgentApprovalCheckpointStatus>,
        limit: usize,
    ) -> Result<Vec<AgentApprovalCheckpoint>, RepositoryError> {
        if limit == 0 || limit > 1_000 {
            return Err(invalid_repository_write(
                "Agent approval checkpoint list limit is invalid",
            ));
        }
        let mut values = self
            .state
            .read()
            .await
            .checkpoints
            .values()
            .filter(|checkpoint| {
                checkpoint.organization_id == organization_id
                    && checkpoint.execution_id == execution_id
                    && status.is_none_or(|status| checkpoint.status == status)
            })
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| {
            right
                .requested_at
                .cmp(&left.requested_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        values.truncate(limit);
        Ok(values)
    }
}

fn append_resolution(
    state: &mut State,
    checkpoint: &AgentApprovalCheckpoint,
) -> Result<(), RepositoryError> {
    let execution_key = (checkpoint.organization_id, checkpoint.execution_id);
    let mut execution = state
        .executions
        .get(&execution_key)
        .cloned()
        .ok_or_else(|| corrupt("Agent approval execution is missing"))?;
    if execution.conversation_id != checkpoint.conversation_id {
        return Err(corrupt(
            "Agent approval checkpoint changed its conversation",
        ));
    }
    let conversation_key = (checkpoint.organization_id, checkpoint.conversation_id);
    let mut conversation = state
        .conversations
        .get(&conversation_key)
        .cloned()
        .ok_or_else(|| corrupt("Agent approval conversation is missing"))?;
    let draft = checkpoint
        .resolution_event_draft()
        .map_err(invalid_repository_write)?;
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
    .map_err(invalid_repository_write)?;
    let event_key = (event.organization_id, event.conversation_id, event.sequence);
    if state.events.contains_key(&event_key) {
        return Err(corrupt(
            "Agent approval event sequence is already committed",
        ));
    }
    state.executions.insert(execution_key, execution);
    state.conversations.insert(conversation_key, conversation);
    state.events.insert(event_key, event);
    Ok(())
}
