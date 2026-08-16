use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId};

pub(super) fn environment(
    project_id: ProjectId,
    environment_id: EnvironmentId,
    evaluator: &ResourceAccessEvaluator,
) -> ApplicationResult<()> {
    if evaluator.allows(ResourceGrantScope::Environment {
        project_id,
        environment_id,
    }) {
        return Ok(());
    }
    Err(environment_not_found())
}

pub(super) fn environment_not_found() -> ApplicationError {
    ApplicationError::NotFound("Durable Cell environment not found".into())
}

pub(super) fn application_not_found() -> ApplicationError {
    ApplicationError::NotFound("Durable Cell application not found".into())
}

pub(super) fn revision_not_found() -> ApplicationError {
    ApplicationError::NotFound("Durable Cell application revision not found".into())
}

pub(super) fn build_run_not_found() -> ApplicationError {
    ApplicationError::NotFound("Durable Cell BuildRun not found".into())
}
