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
    Err(ApplicationError::NotFound(
        "UserFile project not found".into(),
    ))
}

pub(super) fn organization_quota(evaluator: &ResourceAccessEvaluator) -> ApplicationResult<()> {
    if evaluator.is_organization_wide() {
        return Ok(());
    }
    Err(ApplicationError::NotFound(
        "UserFile organization quota not found".into(),
    ))
}

pub(super) fn user_file_not_found() -> ApplicationError {
    ApplicationError::NotFound("UserFile not found".into())
}
