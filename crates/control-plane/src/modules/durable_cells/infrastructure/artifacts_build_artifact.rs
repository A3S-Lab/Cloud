use crate::modules::artifacts::domain::{BuildRunStatus, IBuildRunRepository};
use crate::modules::durable_cells::application::{
    DurableCellBuildArtifact, DurableCellBuildArtifactRequest, IDurableCellBuildArtifactPort,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::sync::Arc;

/// Anti-corruption adapter from the Artifacts BuildRun authority to the
/// Durable Cells consumer-owned artifact port.
///
/// No BuildRun aggregate or Artifacts repository crosses into Durable Cells
/// Application. This adapter admits only a successful, typed published
/// output and keeps owner lifecycle/integrity interpretation here.
pub struct ArtifactsDurableCellBuildArtifactAdapter {
    builds: Arc<dyn IBuildRunRepository>,
}

impl ArtifactsDurableCellBuildArtifactAdapter {
    pub fn new(builds: Arc<dyn IBuildRunRepository>) -> Self {
        Self { builds }
    }
}

#[async_trait]
impl IDurableCellBuildArtifactPort for ArtifactsDurableCellBuildArtifactAdapter {
    async fn find_published_bundle(
        &self,
        request: &DurableCellBuildArtifactRequest,
    ) -> ApplicationResult<DurableCellBuildArtifact> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let build = match self
            .builds
            .find(request.organization_id, request.build_run_id)
            .await
        {
            Ok(build) => build,
            Err(RepositoryError::NotFound) => {
                return Err(ApplicationError::NotFound(
                    "Durable Cell BuildRun not found".into(),
                ))
            }
            Err(error) => return Err(error.into()),
        };
        if build.organization_id != request.organization_id
            || build.id != request.build_run_id
            || build.project_id() != Some(request.project_id)
            || build.environment_id() != Some(request.environment_id)
        {
            return Err(ApplicationError::NotFound(
                "Durable Cell BuildRun not found".into(),
            ));
        }
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
        let output = build.published_output.ok_or_else(|| {
            ApplicationError::Invalid(
                "Durable Cell application BuildRun has no typed published bundle".into(),
            )
        })?;
        let artifact = DurableCellBuildArtifact {
            organization_id: build.organization_id,
            project_id: request.project_id,
            environment_id: request.environment_id,
            build_run_id: build.id,
            build_run_version: build.aggregate_version,
            uri: output.uri,
            digest: output.digest,
            media_type: output.media_type,
            size_bytes: output.size_bytes,
        };
        artifact.validate().map_err(|error| {
            ApplicationError::Internal(format!(
                "Artifacts returned an invalid Durable Cell bundle projection: {error}"
            ))
        })?;
        Ok(artifact)
    }
}
