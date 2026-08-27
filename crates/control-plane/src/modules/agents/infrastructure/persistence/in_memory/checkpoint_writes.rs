use super::*;
use crate::modules::agents::domain::{
    AgentConversationStatus, AgentExecution, AgentExecutionCheckpoint,
    AgentExecutionCheckpointWrite, AgentExecutionEvent, AgentExecutionTelemetryCorrelation,
    AgentExecutionWrite, AgentExecutionWriteReference, CommitAgentExecutionCheckpointWrite,
    ForkAgentExecutionWrite, IAgentExecutionCheckpointRepository,
};
use crate::modules::shared_kernel::domain::{
    AgentExecutionCheckpointId, AgentExecutionId, IdempotencyRequest, OrganizationId,
    RepositoryError,
};
use async_trait::async_trait;

#[async_trait]
impl IAgentExecutionCheckpointRepository for InMemoryAgentRepository {
    async fn commit_execution_checkpoint(
        &self,
        write: CommitAgentExecutionCheckpointWrite,
    ) -> Result<AgentExecutionCheckpointWrite, RepositoryError> {
        write.validate().map_err(invalid_repository_write)?;
        let mut state = self.state.write().await;
        if let Some(response) = replay(&state, &write.idempotency)? {
            let IdempotencyResponse::ExecutionCheckpoint(organization_id, checkpoint_id) = response
            else {
                return Err(corrupt("Agent checkpoint replay type changed"));
            };
            let checkpoint = state
                .execution_checkpoints
                .get(&(organization_id, checkpoint_id))
                .cloned()
                .ok_or_else(|| corrupt("Agent checkpoint replay target is missing"))?;
            return Ok(AgentExecutionCheckpointWrite {
                checkpoint,
                replayed: true,
            });
        }

        let key = (write.checkpoint.organization_id, write.checkpoint.id);
        if let Some(existing) = state.execution_checkpoints.get(&key).cloned() {
            if existing != write.checkpoint {
                return Err(RepositoryError::Conflict(
                    "Agent checkpoint identity is already bound to different content".into(),
                ));
            }
            store_replay(
                &mut state,
                write.idempotency,
                IdempotencyResponse::ExecutionCheckpoint(key.0, key.1),
            );
            return Ok(AgentExecutionCheckpointWrite {
                checkpoint: existing,
                replayed: true,
            });
        }

        validate_checkpoint_authority(&state, &write.checkpoint)?;
        state
            .execution_checkpoints
            .insert(key, write.checkpoint.clone());
        store_replay(
            &mut state,
            write.idempotency,
            IdempotencyResponse::ExecutionCheckpoint(key.0, key.1),
        );
        state.outbox.push(write.event);
        Ok(AgentExecutionCheckpointWrite {
            checkpoint: write.checkpoint,
            replayed: false,
        })
    }

    async fn fork_execution(
        &self,
        write: ForkAgentExecutionWrite,
    ) -> Result<AgentExecutionWrite, RepositoryError> {
        write.validate().map_err(invalid_repository_write)?;
        let mut state = self.state.write().await;
        if let Some(response) = replay(&state, &write.idempotency)? {
            let IdempotencyResponse::Execution(reference) = response else {
                return Err(corrupt("Agent execution fork replay type changed"));
            };
            return replay_execution(&state, reference);
        }

        let lineage = write
            .execution
            .lineage
            .as_ref()
            .ok_or_else(|| corrupt("Agent execution fork lineage is missing"))?;
        let checkpoint = state
            .execution_checkpoints
            .get(&(
                write.execution.organization_id,
                lineage.parent_checkpoint_id,
            ))
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let parent = state
            .executions
            .get(&(write.execution.organization_id, lineage.parent_execution_id))
            .cloned()
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
            || lineage.parent_checkpoint_digest != checkpoint.object.digest
            || checkpoint.execution_id != parent.id
        {
            return Err(RepositoryError::Conflict(
                "Agent execution fork changed its committed checkpoint lineage".into(),
            ));
        }

        let conversation_key = (
            write.execution.organization_id,
            write.execution.conversation_id,
        );
        let mut conversation = state
            .conversations
            .get(&conversation_key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if conversation.status != AgentConversationStatus::Active {
            return Err(RepositoryError::Conflict(
                "closed Agent conversation cannot fork an execution".into(),
            ));
        }
        let execution_key = (write.execution.organization_id, write.execution.id);
        if state.executions.contains_key(&execution_key)
            || state
                .executions
                .values()
                .any(|execution| execution.operation_id == write.execution.operation_id)
        {
            return Err(RepositoryError::Conflict(
                "Agent execution identity is already in use".into(),
            ));
        }
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
        let event_key = (
            initial_event.organization_id,
            initial_event.conversation_id,
            initial_event.sequence,
        );
        if state.events.contains_key(&event_key) {
            return Err(corrupt("Agent event sequence is already committed"));
        }

        let reference = AgentExecutionWriteReference {
            organization_id: write.execution.organization_id,
            execution_id: write.execution.id,
        };
        state
            .conversations
            .insert(conversation_key, conversation.clone());
        state
            .executions
            .insert(execution_key, write.execution.clone());
        state.events.insert(event_key, initial_event);
        store_replay(
            &mut state,
            write.idempotency,
            IdempotencyResponse::Execution(reference),
        );
        state.outbox.push(write.event);
        Ok(AgentExecutionWrite {
            conversation,
            execution: write.execution,
            replayed: false,
        })
    }

    async fn replay_execution_checkpoint(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AgentExecutionCheckpoint>, RepositoryError> {
        let state = self.state.read().await;
        let Some(response) = replay(&state, idempotency)? else {
            return Ok(None);
        };
        let IdempotencyResponse::ExecutionCheckpoint(organization_id, checkpoint_id) = response
        else {
            return Err(corrupt("Agent checkpoint replay type changed"));
        };
        state
            .execution_checkpoints
            .get(&(organization_id, checkpoint_id))
            .cloned()
            .map(Some)
            .ok_or_else(|| corrupt("Agent checkpoint replay target is missing"))
    }

    async fn find_execution_checkpoint(
        &self,
        organization_id: OrganizationId,
        checkpoint_id: AgentExecutionCheckpointId,
    ) -> Result<Option<AgentExecutionCheckpoint>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .execution_checkpoints
            .get(&(organization_id, checkpoint_id))
            .cloned())
    }

    async fn list_execution_checkpoints(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
        limit: usize,
    ) -> Result<Vec<AgentExecutionCheckpoint>, RepositoryError> {
        if limit == 0 || limit > 1_000 {
            return Err(RepositoryError::Storage(
                "Agent execution checkpoint list limit is invalid".into(),
            ));
        }
        let mut checkpoints = self
            .state
            .read()
            .await
            .execution_checkpoints
            .values()
            .filter(|checkpoint| {
                checkpoint.organization_id == organization_id
                    && checkpoint.execution_id == execution_id
            })
            .cloned()
            .collect::<Vec<_>>();
        checkpoints.sort_by_key(|checkpoint| {
            std::cmp::Reverse((checkpoint.through_event_sequence, checkpoint.id))
        });
        checkpoints.truncate(limit);
        Ok(checkpoints)
    }

    async fn list_execution_trajectory_events(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
        after_sequence: Option<u64>,
        through_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AgentExecutionEvent>, RepositoryError> {
        if limit == 0 || limit > 1_001 {
            return Err(RepositoryError::Storage(
                "Agent trajectory event limit is invalid".into(),
            ));
        }
        let after_sequence = after_sequence.unwrap_or(0);
        let through_sequence = through_sequence.unwrap_or(u64::MAX);
        Ok(self
            .state
            .read()
            .await
            .events
            .values()
            .filter(|event| {
                event.organization_id == organization_id
                    && event.execution_id == execution_id
                    && event.sequence > after_sequence
                    && event.sequence <= through_sequence
            })
            .take(limit)
            .cloned()
            .collect())
    }
}

fn validate_checkpoint_authority(
    state: &State,
    checkpoint: &AgentExecutionCheckpoint,
) -> Result<(), RepositoryError> {
    let execution = state
        .executions
        .get(&(checkpoint.organization_id, checkpoint.execution_id))
        .ok_or(RepositoryError::NotFound)?;
    let conversation = state
        .conversations
        .get(&(checkpoint.organization_id, checkpoint.conversation_id))
        .ok_or(RepositoryError::NotFound)?;
    let boundary = state
        .events
        .get(&(
            checkpoint.organization_id,
            checkpoint.conversation_id,
            checkpoint.through_event_sequence,
        ))
        .ok_or(RepositoryError::NotFound)?;
    if execution.conversation_id != checkpoint.conversation_id
        || conversation.project_id != checkpoint.project_id
        || conversation.environment_id != checkpoint.environment_id
        || boundary.execution_id != checkpoint.execution_id
        || boundary.occurred_at != checkpoint.captured_at
        || execution.agent.artifact_digest() != &checkpoint.agent_artifact_digest
        || execution.provider.profile_digest() != checkpoint.provider_profile_digest.as_str()
        || execution
            .code
            .as_ref()
            .ok_or(RepositoryError::NotFound)?
            .require_invocation_profile()
            .map_err(RepositoryError::Conflict)?
            .digest()
            .map_err(RepositoryError::Conflict)?
            != checkpoint.invocation_profile_digest.as_str()
        || AgentExecutionTelemetryCorrelation::from_execution(execution)
            .map_err(RepositoryError::Conflict)?
            != checkpoint.telemetry_correlation
    {
        return Err(RepositoryError::Conflict(
            "Agent checkpoint changed its execution or event authority".into(),
        ));
    }
    Ok(())
}

fn replay_execution(
    state: &State,
    reference: AgentExecutionWriteReference,
) -> Result<AgentExecutionWrite, RepositoryError> {
    let execution = state
        .executions
        .get(&(reference.organization_id, reference.execution_id))
        .cloned()
        .ok_or_else(|| corrupt("Agent execution fork replay target is missing"))?;
    let conversation = state
        .conversations
        .get(&(execution.organization_id, execution.conversation_id))
        .cloned()
        .ok_or_else(|| corrupt("Agent execution fork conversation is missing"))?;
    Ok(AgentExecutionWrite {
        conversation,
        execution,
        replayed: true,
    })
}
