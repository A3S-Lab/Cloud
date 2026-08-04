use super::{CreateAgentConversation, CreateAgentConversationResult};
use crate::modules::agents::application::support::{idempotency, validate_request_id};
use crate::modules::agents::domain::{
    AgentConversation, AgentConversationCreated, CreateAgentConversationWrite, IAgentRepository,
};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::AgentConversationId;
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CreateAgentConversationHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    agents: Arc<dyn IAgentRepository>,
}

impl CreateAgentConversationHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        agents: Arc<dyn IAgentRepository>,
    ) -> Self {
        Self {
            environments,
            agents,
        }
    }
}

impl CommandHandler<CreateAgentConversation> for CreateAgentConversationHandler {
    fn execute(
        &self,
        command: CreateAgentConversation,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<CreateAgentConversationResult>>,
    > {
        let environments = Arc::clone(&self.environments);
        let agents = Arc::clone(&self.agents);
        Box::pin(async move {
            if let Err(error) = validate_request_id(command.request_id) {
                return Ok(Err(error));
            }
            let idempotency = match idempotency(
                format!(
                    "organizations/{}/projects/{}/environments/{}/agent-conversations",
                    command.organization_id, command.project_id, command.environment_id
                ),
                command.idempotency_key,
                &serde_json::json!({
                    "organizationId": command.organization_id,
                    "projectId": command.project_id,
                    "environmentId": command.environment_id,
                }),
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(error)),
            };
            match agents.replay_conversation(&idempotency).await {
                Ok(Some(conversation))
                    if conversation.organization_id == command.organization_id
                        && conversation.project_id == command.project_id
                        && conversation.environment_id == command.environment_id =>
                {
                    return Ok(Ok(CreateAgentConversationResult {
                        conversation,
                        replayed: true,
                    }));
                }
                Ok(Some(_)) => {
                    return Err(a3s_boot::BootError::Internal(
                        "Agent conversation replay changed its identity".into(),
                    ));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            match environments
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            }
            let conversation = match AgentConversation::create(
                command.organization_id,
                command.project_id,
                command.environment_id,
                AgentConversationId::new(),
                command.requested_at,
            ) {
                Ok(conversation) => conversation,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = AgentConversationCreated::envelope(&conversation, command.request_id)
                .map_err(|error| a3s_boot::BootError::Internal(error.to_string()))?;
            match agents
                .create_conversation(CreateAgentConversationWrite {
                    conversation,
                    event,
                    idempotency,
                })
                .await
            {
                Ok(write) => Ok(Ok(CreateAgentConversationResult {
                    conversation: write.conversation,
                    replayed: write.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
