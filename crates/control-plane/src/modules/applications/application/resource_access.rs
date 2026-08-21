use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId};

pub(super) fn project(
    project_id: ProjectId,
    evaluator: &ResourceAccessEvaluator,
) -> ApplicationResult<()> {
    if evaluator.allows(ResourceGrantScope::Project { project_id }) {
        return Ok(());
    }
    Err(project_not_found())
}

pub(super) fn project_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application project not found".into())
}

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
    Err(ApplicationError::NotFound(
        "Application environment not found".into(),
    ))
}

pub(super) fn application_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application not found".into())
}

pub(super) fn release_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application release not found".into())
}
