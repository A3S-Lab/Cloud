use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::ProjectId;

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

pub(super) fn application_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application not found".into())
}

pub(super) fn release_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application release not found".into())
}
