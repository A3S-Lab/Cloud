use super::{StartAgentExecution, StartAgentExecutionResult};
use crate::modules::agents::application::resource_access::AgentResourceAccess;
use crate::modules::agents::application::support::{idempotency, validate_request_id};
use crate::modules::agents::domain::{
    AgentConversationStatus, AgentEventContent, AgentExecution, AgentExecutionEventDraft,
    AgentExecutionEventKind, AgentExecutionProviderRegistry, AgentExecutionStarted,
    AgentReleaseBinding, IAgentRepository, StartAgentExecutionWrite,
};
use crate::modules::artifacts::IHostedArtifactQueryPort;
use crate::modules::assets::{load_deployable_agent_release, IAssetRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{AgentExecutionId, OperationId, Sha256Digest};
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct StartAgentExecutionHandler {
    agents: Arc<dyn IAgentRepository>,
    assets: Arc<dyn IAssetRepository>,
    artifacts: Arc<dyn IHostedArtifactQueryPort>,
    providers: Arc<dyn AgentExecutionProviderRegistry>,
}

impl StartAgentExecutionHandler {
    pub fn new(
        agents: Arc<dyn IAgentRepository>,
        assets: Arc<dyn IAssetRepository>,
        artifacts: Arc<dyn IHostedArtifactQueryPort>,
        providers: Arc<dyn AgentExecutionProviderRegistry>,
    ) -> Self {
        Self {
            agents,
            assets,
            artifacts,
            providers,
        }
    }
}

impl CommandHandler<StartAgentExecution> for StartAgentExecutionHandler {
    fn execute(
        &self,
        command: StartAgentExecution,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<StartAgentExecutionResult>>>
    {
        let agents = Arc::clone(&self.agents);
        let assets = Arc::clone(&self.assets);
        let artifacts = Arc::clone(&self.artifacts);
        let providers = Arc::clone(&self.providers);
        Box::pin(async move {
            if let Err(error) = validate_request_id(command.request_id) {
                return Ok(Err(error));
            }
            let conversation = match AgentResourceAccess::new(Arc::clone(&agents))
                .conversation(
                    command.organization_id,
                    command.conversation_id,
                    &command.resource_access,
                )
                .await
            {
                Ok(conversation) => conversation,
                Err(error) => return Ok(Err(error)),
            };
            let provider = match providers.provider_by_kind(&command.provider_kind) {
                Ok(provider) => provider,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let idempotency = match idempotency(
                format!(
                    "organizations/{}/agent-conversations/{}/executions",
                    command.organization_id, command.conversation_id
                ),
                command.idempotency_key,
                &serde_json::json!({
                    "organizationId": command.organization_id,
                    "conversationId": command.conversation_id,
                    "agentAssetId": command.agent_asset_id,
                    "agentAssetReleaseId": command.agent_asset_release_id,
                    "providerKind": provider.profile().kind(),
                    "input": &command.input,
                }),
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(error)),
            };
            match agents.replay_execution(&idempotency).await {
                Ok(Some(execution))
                    if execution.organization_id == command.organization_id
                        && execution.conversation_id == command.conversation_id
                        && execution.agent.asset_id() == command.agent_asset_id
                        && execution.agent.asset_release_id() == command.agent_asset_release_id
                        && &execution.provider == provider.profile() =>
                {
                    return Ok(Ok(StartAgentExecutionResult {
                        conversation,
                        execution,
                        replayed: true,
                    }));
                }
                Ok(Some(_)) => {
                    return Err(a3s_boot::BootError::Internal(
                        "Agent execution replay changed its immutable identity".into(),
                    ));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            if conversation.status != AgentConversationStatus::Active {
                return Ok(Err(ApplicationError::Conflict(
                    "closed Agent conversation cannot start an execution".into(),
                )));
            }
            let deployable = match load_deployable_agent_release(
                assets.as_ref(),
                artifacts.as_ref(),
                command.organization_id,
                command.agent_asset_id,
                command.agent_asset_release_id,
            )
            .await
            {
                Ok(deployable) => deployable,
                Err(error) => return Ok(Err(error)),
            };
            let binding = match AgentReleaseBinding::new(
                command.organization_id,
                deployable.asset_id(),
                deployable.asset_release_id(),
                deployable.build_run_id(),
                deployable.artifact_uri(),
                match Sha256Digest::parse(deployable.artifact_digest()) {
                    Ok(digest) => digest,
                    Err(error) => return Ok(Err(ApplicationError::Internal(error))),
                },
                deployable.artifact_media_type(),
                deployable.artifact_size_bytes(),
            ) {
                Ok(binding) => binding,
                Err(error) => return Ok(Err(ApplicationError::Internal(error))),
            };
            let execution = match AgentExecution::create_with_provider(
                command.organization_id,
                conversation.id,
                AgentExecutionId::new(),
                OperationId::new(),
                binding,
                provider.profile().clone(),
                command.requested_at,
            ) {
                Ok(execution) => execution,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let initial_event = match AgentExecutionEventDraft::new(
                AgentExecutionEventKind::ExecutionRequested,
                match AgentEventContent::inline_json(command.input) {
                    Ok(content) => content,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                },
                command.requested_at,
            ) {
                Ok(event) => event,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = AgentExecutionStarted::envelope(&execution, command.request_id)
                .map_err(|error| a3s_boot::BootError::Internal(error.to_string()))?;
            match agents
                .start_execution(StartAgentExecutionWrite {
                    execution,
                    initial_event,
                    event,
                    idempotency,
                })
                .await
            {
                Ok(write) => Ok(Ok(StartAgentExecutionResult {
                    conversation: write.conversation,
                    execution: write.execution,
                    replayed: write.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
