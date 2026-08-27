use super::{CaptureAgentExecutionCheckpoint, CaptureAgentExecutionCheckpointResult};
use crate::modules::agents::application::resource_access::AgentResourceAccess;
use crate::modules::agents::application::support::{
    checkpoint_object_error, idempotency, load_checkpoint_snapshot, validate_request_id,
};
use crate::modules::agents::domain::{
    AgentExecutionCheckpoint, AgentExecutionCheckpointCommitted,
    CommitAgentExecutionCheckpointWrite, IAgentExecutionCheckpointObjectStore, IAgentRepository,
    MAX_AGENT_EXECUTION_CHECKPOINT_EVENTS,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CaptureAgentExecutionCheckpointHandler {
    agents: Arc<dyn IAgentRepository>,
    objects: Arc<dyn IAgentExecutionCheckpointObjectStore>,
}

impl CaptureAgentExecutionCheckpointHandler {
    pub fn new(
        agents: Arc<dyn IAgentRepository>,
        objects: Arc<dyn IAgentExecutionCheckpointObjectStore>,
    ) -> Self {
        Self { agents, objects }
    }
}

impl CommandHandler<CaptureAgentExecutionCheckpoint> for CaptureAgentExecutionCheckpointHandler {
    fn execute(
        &self,
        command: CaptureAgentExecutionCheckpoint,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<CaptureAgentExecutionCheckpointResult>>,
    > {
        let agents = Arc::clone(&self.agents);
        let objects = Arc::clone(&self.objects);
        Box::pin(async move {
            if let Err(error) = validate_request_id(command.request_id) {
                return Ok(Err(error));
            }
            if command.through_event_sequence == Some(0) {
                return Ok(Err(ApplicationError::Invalid(
                    "Agent checkpoint event sequence must be positive".into(),
                )));
            }
            let access = match AgentResourceAccess::new(Arc::clone(&agents))
                .execution(
                    command.organization_id,
                    command.execution_id,
                    &command.resource_access,
                )
                .await
            {
                Ok(access) => access,
                Err(error) => return Ok(Err(error)),
            };
            let idempotency = match idempotency(
                format!(
                    "organizations/{}/agent-executions/{}/checkpoints",
                    command.organization_id, command.execution_id
                ),
                command.idempotency_key,
                &serde_json::json!({
                    "organizationId": command.organization_id,
                    "executionId": command.execution_id,
                    "throughEventSequence": command.through_event_sequence,
                }),
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(error)),
            };
            match agents.replay_execution_checkpoint(&idempotency).await {
                Ok(Some(checkpoint))
                    if checkpoint.organization_id == command.organization_id
                        && checkpoint.execution_id == access.execution.id
                        && command.through_event_sequence.is_none_or(|sequence| {
                            sequence == checkpoint.through_event_sequence
                        }) =>
                {
                    if let Err(error) = load_checkpoint_snapshot(objects, &checkpoint).await {
                        return Ok(Err(error));
                    }
                    return Ok(Ok(CaptureAgentExecutionCheckpointResult {
                        checkpoint,
                        replayed: true,
                    }));
                }
                Ok(Some(_)) => {
                    return Err(a3s_boot::BootError::Internal(
                        "Agent checkpoint replay changed its immutable identity".into(),
                    ));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let events = match agents
                .list_execution_trajectory_events(
                    command.organization_id,
                    command.execution_id,
                    None,
                    command.through_event_sequence,
                    MAX_AGENT_EXECUTION_CHECKPOINT_EVENTS + 1,
                )
                .await
            {
                Ok(events) => events,
                Err(error) => return Ok(Err(error.into())),
            };
            if events.len() > MAX_AGENT_EXECUTION_CHECKPOINT_EVENTS {
                return Ok(Err(ApplicationError::Conflict(format!(
                    "Agent trajectory exceeds the {MAX_AGENT_EXECUTION_CHECKPOINT_EVENTS}-event checkpoint limit"
                ))));
            }
            if events.is_empty()
                || command.through_event_sequence.is_some_and(|sequence| {
                    events.last().is_none_or(|event| event.sequence != sequence)
                })
            {
                return Ok(Err(ApplicationError::NotFound(
                    "Agent checkpoint boundary event not found".into(),
                )));
            }
            let boundary_sequence = events.last().map(|event| event.sequence).ok_or_else(|| {
                a3s_boot::BootError::Internal(
                    "validated Agent checkpoint trajectory has no boundary".into(),
                )
            })?;
            let checkpoint_id =
                AgentExecutionCheckpoint::derive_id(access.execution.id, boundary_sequence);
            match agents
                .find_execution_checkpoint(command.organization_id, checkpoint_id)
                .await
            {
                Ok(Some(checkpoint))
                    if checkpoint.execution_id == access.execution.id
                        && checkpoint.conversation_id == access.conversation.id
                        && checkpoint.through_event_sequence == boundary_sequence =>
                {
                    if let Err(error) =
                        load_checkpoint_snapshot(Arc::clone(&objects), &checkpoint).await
                    {
                        return Ok(Err(error));
                    }
                    let event = AgentExecutionCheckpointCommitted::envelope(
                        &checkpoint,
                        command.request_id,
                    )
                    .map_err(|error| a3s_boot::BootError::Internal(error.to_string()))?;
                    return match agents
                        .commit_execution_checkpoint(CommitAgentExecutionCheckpointWrite {
                            checkpoint,
                            event,
                            idempotency,
                        })
                        .await
                    {
                        Ok(write) => Ok(Ok(CaptureAgentExecutionCheckpointResult {
                            checkpoint: write.checkpoint,
                            replayed: write.replayed,
                        })),
                        Err(error) => Ok(Err(error.into())),
                    };
                }
                Ok(Some(_)) => {
                    return Err(a3s_boot::BootError::Internal(
                        "Agent checkpoint identity changed its immutable trajectory".into(),
                    ));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let parent =
                if let Some(lineage) = &access.execution.lineage {
                    let parent_checkpoint = match agents
                        .find_execution_checkpoint(
                            command.organization_id,
                            lineage.parent_checkpoint_id,
                        )
                        .await
                    {
                        Ok(Some(checkpoint)) => checkpoint,
                        Ok(None) | Err(RepositoryError::NotFound) => {
                            return Err(a3s_boot::BootError::Internal(
                                "Agent fork lineage checkpoint is missing".into(),
                            ));
                        }
                        Err(error) => return Ok(Err(error.into())),
                    };
                    let parent_snapshot =
                        match load_checkpoint_snapshot(Arc::clone(&objects), &parent_checkpoint)
                            .await
                        {
                            Ok(snapshot) => snapshot,
                            Err(error) => return Ok(Err(error)),
                        };
                    Some((parent_checkpoint, parent_snapshot))
                } else {
                    None
                };
            let capture = match parent.as_ref() {
                Some((parent_checkpoint, parent_snapshot)) => {
                    AgentExecutionCheckpoint::capture_with_parent(
                        &access.conversation,
                        &access.execution,
                        &events,
                        parent_checkpoint,
                        parent_snapshot,
                    )
                }
                None => AgentExecutionCheckpoint::capture(
                    &access.conversation,
                    &access.execution,
                    &events,
                ),
            };
            let captured = match capture {
                Ok(captured) => captured,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            if let Err(error) = objects
                .put(&captured.checkpoint.object, captured.bytes)
                .await
            {
                return Ok(Err(checkpoint_object_error(error)));
            }
            let event = AgentExecutionCheckpointCommitted::envelope(
                &captured.checkpoint,
                command.request_id,
            )
            .map_err(|error| a3s_boot::BootError::Internal(error.to_string()))?;
            match agents
                .commit_execution_checkpoint(CommitAgentExecutionCheckpointWrite {
                    checkpoint: captured.checkpoint,
                    event,
                    idempotency,
                })
                .await
            {
                Ok(write) => Ok(Ok(CaptureAgentExecutionCheckpointResult {
                    checkpoint: write.checkpoint,
                    replayed: write.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
