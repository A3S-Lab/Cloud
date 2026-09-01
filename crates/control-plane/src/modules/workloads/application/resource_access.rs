use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, OrganizationId, ProjectId, RepositoryError, WorkloadId,
};
use crate::modules::workloads::domain::entities::{Deployment, Workload};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use std::collections::BTreeSet;
use std::sync::Arc;

/// One Workloads visibility selector projected from an Identity decision.
///
/// Project selectors include descendant environments; environment selectors
/// expose only one exact environment. Node selectors have no ownership meaning
/// in Workloads and are discarded by the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum WorkloadAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl WorkloadAccessScope {
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

/// Workloads-owned projection of an already-authorized request.
///
/// Identity remains the authentication and authorization authority. Entry
/// adapters narrow that decision into this immutable value, while Workloads
/// resolves its own resource ownership and conceals missing and denied records
/// identically without importing Identity policy vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<WorkloadAccessScope>,
}

impl WorkloadAccess {
    pub(crate) fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub(crate) fn restricted(
        granted_scopes: impl IntoIterator<Item = WorkloadAccessScope>,
    ) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
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

/// Resolves indirect Workloads identifiers through the owning repository before authorization.
///
/// Workloads owns resource-to-environment resolution and evaluates only its local access value.
/// This avoids both a second resource ownership registry and a duplicated Identity policy engine.
#[derive(Clone)]
pub(crate) struct WorkloadResourceResolver {
    workloads: Arc<dyn IWorkloadRepository>,
}

impl WorkloadResourceResolver {
    pub fn new(workloads: Arc<dyn IWorkloadRepository>) -> Self {
        Self { workloads }
    }

    pub async fn workload(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        access: &WorkloadAccess,
    ) -> ApplicationResult<Workload> {
        let workload = self
            .workloads
            .find_workload(organization_id, workload_id)
            .await
            .map_err(|error| map_repository_error(error, "workload not found"))?;
        if !access.environment_is_visible(workload.project_id, workload.environment_id) {
            return Err(ApplicationError::NotFound("workload not found".into()));
        }
        Ok(workload)
    }

    pub async fn deployment(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
        access: &WorkloadAccess,
    ) -> ApplicationResult<Deployment> {
        let deployment = self
            .workloads
            .find_deployment(organization_id, deployment_id)
            .await
            .map_err(|error| map_repository_error(error, "deployment not found"))?;
        let workload = self
            .workloads
            .find_workload(organization_id, deployment.workload_id)
            .await
            .map_err(|error| map_repository_error(error, "deployment not found"))?;
        if !access.environment_is_visible(workload.project_id, workload.environment_id) {
            return Err(ApplicationError::NotFound("deployment not found".into()));
        }
        Ok(deployment)
    }
}

fn map_repository_error(error: RepositoryError, not_found: &'static str) -> ApplicationError {
    match error {
        RepositoryError::NotFound => ApplicationError::NotFound(not_found.into()),
        error => error.into(),
    }
}

#[cfg(test)]
impl WorkloadAccess {
    pub(crate) fn granted_scopes(&self) -> impl Iterator<Item = WorkloadAccessScope> + '_ {
        self.granted_scopes.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_visibility_is_exact_and_canonicalized() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let exact = WorkloadAccessScope::Environment {
            project_id,
            environment_id,
        };
        let access = WorkloadAccess::restricted([exact, exact]);

        assert_eq!(access.granted_scopes().collect::<Vec<_>>(), [exact]);
        assert!(access.environment_is_visible(project_id, environment_id));
        assert!(!access.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(!access.environment_is_visible(ProjectId::new(), environment_id));
    }

    #[test]
    fn project_visibility_includes_descendant_environments() {
        let project_id = ProjectId::new();
        let access = WorkloadAccess::restricted([WorkloadAccessScope::Project { project_id }]);

        assert!(access.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(!access.environment_is_visible(ProjectId::new(), EnvironmentId::new()));
        assert!(WorkloadAccess::organization_wide()
            .environment_is_visible(ProjectId::new(), EnvironmentId::new()));
    }
}
