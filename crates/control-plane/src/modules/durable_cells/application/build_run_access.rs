use super::resource_access::build_run_not_found;
use crate::modules::artifacts::domain::IBuildRunRepository;
use crate::modules::durable_cells::domain::DurableCellApplicationDefinition;
use crate::modules::shared_kernel::application::ApplicationResult;
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
            Ok(())
        }
        Ok(_) | Err(RepositoryError::NotFound) => Err(build_run_not_found()),
        Err(error) => Err(error.into()),
    }
}
