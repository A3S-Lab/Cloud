use super::resource_access::build_run_not_found;
use crate::modules::artifacts::domain::{BuildRunStatus, IBuildRunRepository};
use crate::modules::durable_cells::domain::{
    DurableCellApplicationDefinition, DURABLE_CELL_BUNDLE_MEDIA_TYPE,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError,
};

pub(super) async fn validate_definition_build_run(
    builds: &dyn IBuildRunRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    definition: &DurableCellApplicationDefinition,
) -> ApplicationResult<()> {
    let build_run_id = definition.spec().build_run_id;
    match builds.find(organization_id, build_run_id).await {
        Ok(build)
            if build.organization_id == organization_id
                && build.id == build_run_id
                && build.project_id() == Some(project_id)
                && build.environment_id() == Some(environment_id) =>
        {
            let build = build.restore().map_err(|error| {
                ApplicationError::Internal(format!(
                    "Durable Cell BuildRun failed integrity validation: {error}"
                ))
            })?;
            if build.status != BuildRunStatus::Succeeded {
                return Err(ApplicationError::Invalid(
                    "Durable Cell application requires a terminally successful BuildRun".into(),
                ));
            }
            let output = build.published_output.as_ref().ok_or_else(|| {
                ApplicationError::Invalid(
                    "Durable Cell application BuildRun has no typed published bundle".into(),
                )
            })?;
            let spec = definition.spec();
            if output.media_type != DURABLE_CELL_BUNDLE_MEDIA_TYPE
                || output.digest != spec.bundle_digest.as_str()
                || output.size_bytes != spec.bundle_size_bytes
            {
                return Err(ApplicationError::Invalid(
                    "Durable Cell application bundle does not match its successful BuildRun output"
                        .into(),
                ));
            }
            Ok(())
        }
        Ok(_) | Err(RepositoryError::NotFound) => Err(build_run_not_found()),
        Err(error) => Err(error.into()),
    }
}
