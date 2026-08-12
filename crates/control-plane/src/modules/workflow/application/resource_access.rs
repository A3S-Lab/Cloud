use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    OntologyId, OrganizationId, ProjectId, RepositoryError, WorkflowDefinitionId, WorkflowGoalId,
    WorkflowRunId,
};
use crate::modules::workflow::domain::{
    IOntologyRepository, IWorkflowDefinitionRepository, IWorkflowGoalRepository,
    IWorkflowRunRepository, Ontology, WorkflowDefinition, WorkflowGoalRecord, WorkflowRunRecord,
};
use std::future::Future;

/// Resolves every indirect Workflow identity through its owning repository before grant
/// evaluation. Revisions and plans inherit their parent aggregate's project identity; callers
/// must authorize that parent before reading the child record.
///
/// An environment grant does not broaden access to these project-scoped aggregates. Missing and
/// denied identifiers intentionally share each aggregate's established not-found contract.
pub(crate) async fn ontology(
    repository: &dyn IOntologyRepository,
    organization_id: OrganizationId,
    ontology_id: OntologyId,
    evaluator: &ResourceAccessEvaluator,
) -> ApplicationResult<Ontology> {
    project_owned(
        repository.find(organization_id, ontology_id),
        |value| value.project_id,
        evaluator,
        "Ontology not found",
    )
    .await
}

pub(crate) async fn workflow_definition(
    repository: &dyn IWorkflowDefinitionRepository,
    organization_id: OrganizationId,
    workflow_definition_id: WorkflowDefinitionId,
    evaluator: &ResourceAccessEvaluator,
) -> ApplicationResult<WorkflowDefinition> {
    project_owned(
        repository.find(organization_id, workflow_definition_id),
        |value| value.project_id,
        evaluator,
        "WorkflowDefinition not found",
    )
    .await
}

pub(crate) async fn workflow_goal(
    repository: &dyn IWorkflowGoalRepository,
    organization_id: OrganizationId,
    workflow_goal_id: WorkflowGoalId,
    evaluator: &ResourceAccessEvaluator,
) -> ApplicationResult<WorkflowGoalRecord> {
    project_owned(
        repository.find(organization_id, workflow_goal_id),
        |value| value.goal.project_id,
        evaluator,
        "WorkflowGoal not found",
    )
    .await
}

pub(crate) async fn workflow_run(
    repository: &dyn IWorkflowRunRepository,
    organization_id: OrganizationId,
    workflow_run_id: WorkflowRunId,
    evaluator: &ResourceAccessEvaluator,
) -> ApplicationResult<WorkflowRunRecord> {
    project_owned(
        repository.find(organization_id, workflow_run_id),
        |value| value.run.project_id,
        evaluator,
        "WorkflowRun not found",
    )
    .await
}

async fn project_owned<T>(
    lookup: impl Future<Output = Result<Option<T>, RepositoryError>>,
    project_id: impl FnOnce(&T) -> ProjectId,
    evaluator: &ResourceAccessEvaluator,
    not_found_message: &'static str,
) -> ApplicationResult<T> {
    let value = match lookup.await {
        Ok(Some(value)) => value,
        Ok(None) | Err(RepositoryError::NotFound) => return Err(not_found(not_found_message)),
        Err(error) => return Err(error.into()),
    };
    authorize_project(project_id(&value), evaluator, not_found_message)?;
    Ok(value)
}

fn authorize_project(
    project_id: ProjectId,
    evaluator: &ResourceAccessEvaluator,
    not_found_message: &'static str,
) -> ApplicationResult<()> {
    if evaluator.allows(ResourceGrantScope::Project { project_id }) {
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
    use crate::modules::shared_kernel::domain::EnvironmentId;

    #[test]
    fn project_aggregates_require_project_authority() {
        let project_id = ProjectId::new();
        assert!(authorize_project(
            project_id,
            &ResourceAccessEvaluator::organization_wide(),
            "not found"
        )
        .is_ok());
        assert!(authorize_project(
            project_id,
            &ResourceAccessEvaluator::restricted([ResourceGrantScope::Project { project_id }]),
            "not found"
        )
        .is_ok());
        assert!(matches!(
            authorize_project(
                project_id,
                &ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
                    project_id,
                    environment_id: EnvironmentId::new(),
                }]),
                "not found",
            ),
            Err(ApplicationError::NotFound(_))
        ));
    }
}
