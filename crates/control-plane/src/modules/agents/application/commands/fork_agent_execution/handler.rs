use super::{ForkAgentExecution, ForkAgentExecutionResult};
use crate::modules::agents::application::resource_access::AgentResourceAccess;
use crate::modules::agents::application::{
    support::{idempotency, load_checkpoint_snapshot, validate_request_id},
    AgentReleaseAdmissionRequest, IAgentReleaseAdmissionPort,
};
use crate::modules::agents::domain::{
    AgentEventContent, AgentExecution, AgentExecutionEventDraft, AgentExecutionEventKind,
    AgentExecutionForked, AgentExecutionProviderRegistry, ForkAgentExecutionWrite,
    IAgentExecutionCheckpointObjectStore, IAgentRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{AgentExecutionId, OperationId, RepositoryError};
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct ForkAgentExecutionHandler {
    agents: Arc<dyn IAgentRepository>,
    objects: Arc<dyn IAgentExecutionCheckpointObjectStore>,
    releases: Arc<dyn IAgentReleaseAdmissionPort>,
    providers: Arc<dyn AgentExecutionProviderRegistry>,
}

impl ForkAgentExecutionHandler {
    pub fn new(
        agents: Arc<dyn IAgentRepository>,
        objects: Arc<dyn IAgentExecutionCheckpointObjectStore>,
        releases: Arc<dyn IAgentReleaseAdmissionPort>,
        providers: Arc<dyn AgentExecutionProviderRegistry>,
    ) -> Self {
        Self {
            agents,
            objects,
            releases,
            providers,
        }
    }
}

impl CommandHandler<ForkAgentExecution> for ForkAgentExecutionHandler {
    fn execute(
        &self,
        command: ForkAgentExecution,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ForkAgentExecutionResult>>>
    {
        let agents = Arc::clone(&self.agents);
        let objects = Arc::clone(&self.objects);
        let releases = Arc::clone(&self.releases);
        let providers = Arc::clone(&self.providers);
        Box::pin(async move {
            if let Err(error) = validate_request_id(command.request_id) {
                return Ok(Err(error));
            }
            let access = match AgentResourceAccess::new(Arc::clone(&agents))
                .execution(
                    command.organization_id,
                    command.parent_execution_id,
                    &command.resource_access,
                )
                .await
            {
                Ok(access) => access,
                Err(error) => return Ok(Err(error)),
            };
            let checkpoint = match agents
                .find_execution_checkpoint(command.organization_id, command.checkpoint_id)
                .await
            {
                Ok(Some(checkpoint))
                    if checkpoint.execution_id == access.execution.id
                        && checkpoint.conversation_id == access.conversation.id =>
                {
                    checkpoint
                }
                Ok(Some(_)) | Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "Agent execution checkpoint not found".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if let Err(error) = load_checkpoint_snapshot(objects, &checkpoint).await {
                return Ok(Err(error));
            }
            let idempotency = match idempotency(
                format!(
                    "organizations/{}/agent-executions/{}/checkpoints/{}/fork",
                    command.organization_id, command.parent_execution_id, command.checkpoint_id
                ),
                command.idempotency_key,
                &serde_json::json!({
                    "organizationId": command.organization_id,
                    "parentExecutionId": command.parent_execution_id,
                    "checkpointId": command.checkpoint_id,
                    "input": &command.input,
                }),
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(error)),
            };
            match agents.replay_execution(&idempotency).await {
                Ok(Some(execution)) => {
                    let request = match agents
                        .find_execution_request(command.organization_id, execution.id)
                        .await
                    {
                        Ok(Some(request)) => request,
                        Ok(None) | Err(RepositoryError::NotFound) => {
                            return Err(a3s_boot::BootError::Internal(
                                "Agent fork replay input is missing".into(),
                            ));
                        }
                        Err(error) => return Ok(Err(error.into())),
                    };
                    if execution.conversation_id != access.conversation.id
                        || execution.lineage.as_ref().is_none_or(|lineage| {
                            lineage.parent_execution_id != access.execution.id
                                || lineage.parent_checkpoint_id != checkpoint.id
                                || lineage.parent_checkpoint_digest != checkpoint.object.digest
                        })
                        || request.content.value() != &command.input
                    {
                        return Err(a3s_boot::BootError::Internal(
                            "Agent fork replay changed its immutable identity".into(),
                        ));
                    }
                    return Ok(Ok(ForkAgentExecutionResult {
                        conversation: access.conversation,
                        execution,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            match providers.provider_by_kind(access.execution.provider.kind()) {
                Ok(provider) if provider.profile() == &access.execution.provider => {}
                Ok(_) | Err(_) => {
                    return Ok(Err(ApplicationError::Conflict(
                        "Agent fork provider profile is no longer available".into(),
                    )));
                }
            };
            let current_agent = match releases
                .admit(AgentReleaseAdmissionRequest {
                    organization_id: command.organization_id,
                    asset_id: access.execution.agent.asset_id(),
                    asset_release_id: access.execution.agent.asset_release_id(),
                })
                .await
            {
                Ok(binding) => binding,
                Err(error) => return Ok(Err(error)),
            };
            if current_agent != access.execution.agent {
                return Ok(Err(ApplicationError::Conflict(
                    "Agent fork release or provider identity changed after checkpoint capture"
                        .into(),
                )));
            }
            let requested_at = command
                .requested_at
                .max(checkpoint.captured_at)
                .max(access.conversation.updated_at);
            let execution = match AgentExecution::fork_from(
                &access.execution,
                &checkpoint,
                AgentExecutionId::new(),
                OperationId::new(),
                requested_at,
            ) {
                Ok(execution) => execution,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let initial_event = match AgentExecutionEventDraft::new(
                AgentExecutionEventKind::ExecutionRequested,
                match AgentEventContent::inline_json(command.input) {
                    Ok(content) => content,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                },
                requested_at,
            ) {
                Ok(event) => event,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = AgentExecutionForked::envelope(&execution, command.request_id)
                .map_err(|error| a3s_boot::BootError::Internal(error.to_string()))?;
            match agents
                .fork_execution(ForkAgentExecutionWrite {
                    execution,
                    initial_event,
                    event,
                    idempotency,
                })
                .await
            {
                Ok(write) => Ok(Ok(ForkAgentExecutionResult {
                    conversation: write.conversation,
                    execution: write.execution,
                    replayed: write.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
