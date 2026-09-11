use crate::modules::agents::domain::{AgentConversation, AgentExecution, IAgentRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    AgentConversationId, AgentExecutionId, EnvironmentId, OrganizationId, ProjectId,
    RepositoryError,
};
use std::collections::BTreeSet;
use std::sync::Arc;

/// One Agents visibility selector projected from an Identity decision.
///
/// Project selectors include descendant environments; environment selectors
/// expose only one exact environment. Node selectors have no ownership meaning
/// for Agents and are discarded by the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AgentAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl AgentAccessScope {
    fn allows(self, project_id: ProjectId, environment_id: EnvironmentId) -> bool {
        match self {
            Self::Project {
                project_id: granted,
            } => granted == project_id,
            Self::Environment {
                project_id: granted_project,
                environment_id: granted_environment,
            } => granted_project == project_id && granted_environment == environment_id,
        }
    }
}

/// Agents-owned projection of an already-authorized request.
///
/// Identity remains the authentication and authorization authority. Entry
/// adapters narrow that decision into this immutable value, while Agents
/// resolves conversation ownership and conceals missing and denied records
/// identically without importing Identity policy vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<AgentAccessScope>,
}

impl AgentAccess {
    pub(crate) fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub(crate) fn restricted(granted_scopes: impl IntoIterator<Item = AgentAccessScope>) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub(crate) const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

    pub(crate) fn granted_scopes(&self) -> impl Iterator<Item = AgentAccessScope> + '_ {
        self.granted_scopes.iter().copied()
    }

    pub(crate) fn environment_is_visible(
        &self,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|scope| scope.allows(project_id, environment_id))
    }
}

/// Resolves indirect Agent identifiers through their canonical conversation before
/// local access evaluation.
///
/// Agents owns resource-to-scope resolution. Missing and denied resources share the
/// same not-found contract at the application boundary.
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
        access: &AgentAccess,
    ) -> ApplicationResult<AgentConversation> {
        let conversation = self
            .load_conversation(
                organization_id,
                conversation_id,
                "Agent conversation not found",
            )
            .await?;
        if !access.environment_is_visible(conversation.project_id, conversation.environment_id) {
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
        access: &AgentAccess,
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
        if !access.environment_is_visible(conversation.project_id, conversation.environment_id) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_visibility_matches_project_and_exact_environment_grants() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let exact = AgentAccess::restricted([AgentAccessScope::Environment {
            project_id,
            environment_id,
        }]);
        assert!(exact.environment_is_visible(project_id, environment_id));
        assert!(!exact.environment_is_visible(project_id, EnvironmentId::new()));

        let project = AgentAccess::restricted([AgentAccessScope::Project { project_id }]);
        assert!(project.environment_is_visible(project_id, environment_id));
        assert!(!project.environment_is_visible(ProjectId::new(), environment_id));
        assert!(AgentAccess::organization_wide()
            .environment_is_visible(ProjectId::new(), EnvironmentId::new()));
    }
}
