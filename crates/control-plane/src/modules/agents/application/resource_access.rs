use crate::modules::agents::domain::{AgentConversation, AgentExecution, IAgentRepository};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    AgentConversationId, AgentExecutionId, OrganizationId, RepositoryError,
};
use std::sync::Arc;

/// Resolves indirect Agent identifiers through their canonical conversation before authorization.
///
/// Identity owns grant semantics; Agents owns resource-to-scope resolution. Keeping that split
/// avoids a second resource ownership registry and makes missing and denied resources
/// indistinguishable at the application boundary.
#[derive(Clone)]
pub(crate) struct AgentResourceAccess {
    agents: Arc<dyn IAgentRepository>,
}

pub(crate) struct AuthorizedAgentExecution {
    pub conversation: AgentConversation,
    pub execution: AgentExecution,
}

impl AgentResourceAccess {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }

    pub async fn conversation(
        &self,
        organization_id: OrganizationId,
        conversation_id: AgentConversationId,
        evaluator: &ResourceAccessEvaluator,
    ) -> ApplicationResult<AgentConversation> {
        let conversation = self
            .load_conversation(
                organization_id,
                conversation_id,
                "Agent conversation not found",
            )
            .await?;
        if !evaluator.allows(conversation_scope(&conversation)) {
            return Err(ApplicationError::NotFound(
                "Agent conversation not found".into(),
            ));
        }
        Ok(conversation)
    }

    pub async fn execution(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
        evaluator: &ResourceAccessEvaluator,
    ) -> ApplicationResult<AuthorizedAgentExecution> {
        let execution = match self
            .agents
            .find_execution(organization_id, execution_id)
            .await
        {
            Ok(Some(execution)) => execution,
            Ok(None) | Err(RepositoryError::NotFound) => {
                return Err(ApplicationError::NotFound(
                    "Agent execution not found".into(),
                ));
            }
            Err(error) => return Err(error.into()),
        };
        let conversation = self
            .load_conversation(
                organization_id,
                execution.conversation_id,
                "Agent execution not found",
            )
            .await?;
        if !evaluator.allows(conversation_scope(&conversation)) {
            return Err(ApplicationError::NotFound(
                "Agent execution not found".into(),
            ));
        }
        Ok(AuthorizedAgentExecution {
            conversation,
            execution,
        })
    }

    async fn load_conversation(
        &self,
        organization_id: OrganizationId,
        conversation_id: AgentConversationId,
        not_found: &'static str,
    ) -> ApplicationResult<AgentConversation> {
        match self
            .agents
            .find_conversation(organization_id, conversation_id)
            .await
        {
            Ok(Some(conversation)) => Ok(conversation),
            Ok(None) | Err(RepositoryError::NotFound) => {
                Err(ApplicationError::NotFound(not_found.into()))
            }
            Err(error) => Err(error.into()),
        }
    }
}

fn conversation_scope(conversation: &AgentConversation) -> ResourceGrantScope {
    ResourceGrantScope::Environment {
        project_id: conversation.project_id,
        environment_id: conversation.environment_id,
    }
}
