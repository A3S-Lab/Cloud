use super::build_artifact_port::{
    DurableCellBuildArtifact, DurableCellBuildArtifactRequest, IDurableCellBuildArtifactPort,
};
use crate::modules::durable_cells::domain::{
    DurableCellApplicationDefinition, DURABLE_CELL_BUNDLE_MEDIA_TYPE,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};

pub(super) async fn validate_definition_build_run(
    builds: &dyn IDurableCellBuildArtifactPort,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    definition: &DurableCellApplicationDefinition,
) -> ApplicationResult<()> {
    require_definition_build_output(
        builds,
        organization_id,
        project_id,
        environment_id,
        definition,
    )
    .await
    .map(drop)
}

pub(super) async fn require_definition_build_output(
    builds: &dyn IDurableCellBuildArtifactPort,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    definition: &DurableCellApplicationDefinition,
) -> ApplicationResult<DurableCellBuildArtifact> {
    let build_run_id = definition.spec().build_run_id;
    let output = builds
        .find_published_bundle(&DurableCellBuildArtifactRequest {
            organization_id,
            project_id,
            environment_id,
            build_run_id,
        })
        .await?;
    let spec = definition.spec();
    if output.organization_id != organization_id
        || output.project_id != project_id
        || output.environment_id != environment_id
        || output.build_run_id != build_run_id
        || output.media_type != DURABLE_CELL_BUNDLE_MEDIA_TYPE
        || output.digest != spec.bundle_digest.as_str()
        || output.size_bytes != spec.bundle_size_bytes
    {
        return Err(ApplicationError::Invalid(
            "Durable Cell application bundle does not match its successful BuildRun output".into(),
        ));
    }
    output
        .validate()
        .map_err(|error| ApplicationError::Internal(format!("invalid BuildRun output: {error}")))?;
    Ok(output)
}
