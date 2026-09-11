use crate::modules::executions::domain::{Execution, IExecutionRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, ExecutionId, OrganizationId, ProjectId, RepositoryError,
};
use std::collections::BTreeSet;
use std::sync::Arc;

/// One Executions visibility selector projected from an Identity decision.
///
/// Project selectors include descendant environments; environment selectors
/// expose only one exact environment. Node selectors have no ownership meaning
/// for Executions and are discarded by the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ExecutionAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl ExecutionAccessScope {
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

/// Executions-owned projection of an already-authorized request.
///
/// Identity remains the authentication and authorization authority. Entry
/// adapters narrow that decision into this immutable value, while Executions
/// resolves its own resource ownership and conceals missing and denied records
/// identically without importing Identity policy vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<ExecutionAccessScope>,
}

impl ExecutionAccess {
    pub(crate) fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub(crate) fn restricted(
        granted_scopes: impl IntoIterator<Item = ExecutionAccessScope>,
    ) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub(crate) const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

    pub(crate) fn granted_scopes(&self) -> impl Iterator<Item = ExecutionAccessScope> + '_ {
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

    /// Project-owned surfaces such as execution templates require a project
    /// grant. An environment-only grant does not broaden project inventory.
    pub(crate) fn project_is_visible(&self, project_id: ProjectId) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .contains(&ExecutionAccessScope::Project { project_id })
    }
}

/// Resolves an indirect Execution identifier through the owning repository before
/// local access evaluation.
///
/// Executions owns the canonical project/environment identity. Missing and denied
/// identifiers intentionally share the same not-found contract.
#[derive(Clone)]
pub(crate) struct ExecutionResourceAccess {
    executions: Arc<dyn IExecutionRepository>,
}

impl ExecutionResourceAccess {
    pub fn new(executions: Arc<dyn IExecutionRepository>) -> Self {
        Self { executions }
    }

    pub async fn execution(
        &self,
        organization_id: OrganizationId,
        execution_id: ExecutionId,
        access: &ExecutionAccess,
    ) -> ApplicationResult<Execution> {
        let execution = match self.executions.find(organization_id, execution_id).await {
            Ok(Some(execution)) => execution,
            Ok(None) | Err(RepositoryError::NotFound) => return Err(not_found()),
            Err(error) => return Err(error.into()),
        };
        if !access.environment_is_visible(execution.project_id, execution.environment_id)
            || execution.is_bound_task()
        {
            return Err(not_found());
        }
        Ok(execution)
    }
}

fn not_found() -> ApplicationError {
    ApplicationError::NotFound("execution not found".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_visibility_matches_project_and_exact_environment_grants() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let exact = ExecutionAccess::restricted([ExecutionAccessScope::Environment {
            project_id,
            environment_id,
        }]);
        assert!(exact.environment_is_visible(project_id, environment_id));
        assert!(!exact.environment_is_visible(project_id, EnvironmentId::new()));

        let project = ExecutionAccess::restricted([ExecutionAccessScope::Project { project_id }]);
        assert!(project.environment_is_visible(project_id, environment_id));
        assert!(!project.environment_is_visible(ProjectId::new(), environment_id));
        assert!(
            ExecutionAccess::organization_wide()
                .environment_is_visible(ProjectId::new(), EnvironmentId::new())
        );
    }
}
