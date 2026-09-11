use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    HumanTaskId, OntologyId, OrganizationId, ProjectId, RepositoryError, WorkflowDefinitionId,
    WorkflowGoalId, WorkflowRunId,
};
use crate::modules::workflow::domain::{
    HumanTaskRecord, IHumanTaskRepository, IOntologyRepository, IWorkflowDefinitionRepository,
    IWorkflowGoalRepository, IWorkflowRunRepository, Ontology, WorkflowDefinition,
    WorkflowGoalRecord, WorkflowRunRecord,
};
use std::collections::BTreeSet;
use std::future::Future;

/// One Workflow visibility selector projected from an Identity decision.
///
/// Workflow aggregates are project-scoped. Environment and Node grants have no
/// ownership meaning here and are discarded by the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum WorkflowAccessScope {
    Project { project_id: ProjectId },
}

/// Workflow-owned projection of an already-authorized request.
///
/// Identity remains the authentication and authorization authority. Entry
/// adapters narrow that decision into this immutable value. An environment
/// grant does not broaden access to these project-scoped aggregates. Missing
/// and denied identifiers share each aggregate's established not-found contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<WorkflowAccessScope>,
}

impl WorkflowAccess {
    pub(crate) fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub(crate) fn restricted(
        granted_scopes: impl IntoIterator<Item = WorkflowAccessScope>,
    ) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub(crate) const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

    pub(crate) fn granted_scopes(&self) -> impl Iterator<Item = WorkflowAccessScope> + '_ {
        self.granted_scopes.iter().copied()
    }

    pub(crate) fn project_is_visible(&self, project_id: ProjectId) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .contains(&WorkflowAccessScope::Project { project_id })
    }
}

/// Resolves every indirect Workflow identity through its owning repository before
/// local access evaluation. Revisions and plans inherit their parent aggregate's
/// project identity; callers must authorize that parent before reading the child.
pub(crate) async fn ontology(
    repository: &dyn IOntologyRepository,
    organization_id: OrganizationId,
    ontology_id: OntologyId,
    access: &WorkflowAccess,
) -> ApplicationResult<Ontology> {
    project_owned(
        repository.find(organization_id, ontology_id),
        |value| value.project_id,
        access,
        "Ontology not found",
    )
    .await
}

pub(crate) async fn workflow_definition(
    repository: &dyn IWorkflowDefinitionRepository,
    organization_id: OrganizationId,
    workflow_definition_id: WorkflowDefinitionId,
    access: &WorkflowAccess,
) -> ApplicationResult<WorkflowDefinition> {
    project_owned(
        repository.find(organization_id, workflow_definition_id),
        |value| value.project_id,
        access,
        "WorkflowDefinition not found",
    )
    .await
}

pub(crate) async fn workflow_goal(
    repository: &dyn IWorkflowGoalRepository,
    organization_id: OrganizationId,
    workflow_goal_id: WorkflowGoalId,
    access: &WorkflowAccess,
) -> ApplicationResult<WorkflowGoalRecord> {
    project_owned(
        repository.find(organization_id, workflow_goal_id),
        |value| value.goal.project_id,
        access,
        "WorkflowGoal not found",
    )
    .await
}

pub(crate) async fn workflow_run(
    repository: &dyn IWorkflowRunRepository,
    organization_id: OrganizationId,
    workflow_run_id: WorkflowRunId,
    access: &WorkflowAccess,
) -> ApplicationResult<WorkflowRunRecord> {
    project_owned(
        repository.find(organization_id, workflow_run_id),
        |value| value.run.project_id,
        access,
        "WorkflowRun not found",
    )
    .await
}

pub(crate) async fn human_task(
    repository: &dyn IHumanTaskRepository,
    organization_id: OrganizationId,
    human_task_id: HumanTaskId,
    access: &WorkflowAccess,
) -> ApplicationResult<HumanTaskRecord> {
    project_owned(
        repository.find_task(organization_id, human_task_id),
        |value| value.task.project_id,
        access,
        "HumanTask not found",
    )
    .await
}

pub(crate) fn human_task_project(
    project_id: ProjectId,
    access: &WorkflowAccess,
) -> ApplicationResult<()> {
    authorize_project(project_id, access, "HumanTask project not found")
}

async fn project_owned<T>(
    lookup: impl Future<Output = Result<Option<T>, RepositoryError>>,
    project_id: impl FnOnce(&T) -> ProjectId,
    access: &WorkflowAccess,
    not_found_message: &'static str,
) -> ApplicationResult<T> {
    let value = match lookup.await {
        Ok(Some(value)) => value,
        Ok(None) | Err(RepositoryError::NotFound) => return Err(not_found(not_found_message)),
        Err(error) => return Err(error.into()),
    };
    authorize_project(project_id(&value), access, not_found_message)?;
    Ok(value)
}

fn authorize_project(
    project_id: ProjectId,
    access: &WorkflowAccess,
    not_found_message: &'static str,
) -> ApplicationResult<()> {
    if access.project_is_visible(project_id) {
        return Ok(());
    }
    Err(not_found(not_found_message))
}

fn not_found(message: &'static str) -> ApplicationError {
    ApplicationError::NotFound(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_aggregates_require_project_authority() {
        let project_id = ProjectId::new();
        assert!(authorize_project(
            project_id,
            &WorkflowAccess::organization_wide(),
            "not found"
        )
        .is_ok());
        assert!(authorize_project(
            project_id,
            &WorkflowAccess::restricted([WorkflowAccessScope::Project { project_id }]),
            "not found"
        )
        .is_ok());
        assert!(matches!(
            authorize_project(
                project_id,
                &WorkflowAccess::restricted([]),
                "not found",
            ),
            Err(ApplicationError::NotFound(_))
        ));
        assert!(!WorkflowAccess::restricted([WorkflowAccessScope::Project {
            project_id: ProjectId::new(),
        }])
        .project_is_visible(project_id));
    }
}
