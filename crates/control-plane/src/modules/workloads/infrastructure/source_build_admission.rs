use crate::modules::artifacts::{BuildRunStatus, IBuildRunRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::sources::domain::ISourceRevisionRepository;
use crate::modules::workloads::application::{
    IWorkloadSourceBuildAdmissionPort, WorkloadSourceBuildAdmissionRequest,
};
use crate::modules::workloads::domain::entities::{
    ExternalBuildReference, OciArtifact, SourceBuildAdmission,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Consumer-owned anti-corruption adapter for Sources and Artifacts owner
/// interfaces. It creates no repository, cache, lifecycle, or retry mechanism.
#[derive(Clone)]
pub struct SourcesArtifactsWorkloadSourceBuildAdmissionAdapter {
    sources: Arc<dyn ISourceRevisionRepository>,
    builds: Arc<dyn IBuildRunRepository>,
}

impl SourcesArtifactsWorkloadSourceBuildAdmissionAdapter {
    pub fn new(
        sources: Arc<dyn ISourceRevisionRepository>,
        builds: Arc<dyn IBuildRunRepository>,
    ) -> Self {
        Self { sources, builds }
    }
}

#[async_trait]
impl IWorkloadSourceBuildAdmissionPort for SourcesArtifactsWorkloadSourceBuildAdmissionAdapter {
    async fn admit(
        &self,
        request: WorkloadSourceBuildAdmissionRequest,
    ) -> ApplicationResult<SourceBuildAdmission> {
        let source = match self
            .sources
            .find(request.organization_id, request.source_revision_id)
            .await
        {
            Ok(source)
                if source.organization_id == request.organization_id
                    && source.project_id == request.project_id
                    && source.environment_id == request.environment_id
                    && source.id == request.source_revision_id =>
            {
                source
            }
            Ok(_) | Err(RepositoryError::NotFound) => {
                return Err(ApplicationError::NotFound(
                    "source revision not found".into(),
                ))
            }
            Err(error) => return Err(error.into()),
        };
        let build = match self
            .builds
            .find_by_source_revision(request.organization_id, source.id)
            .await
        {
            Ok(Some(build))
                if build.organization_id == request.organization_id
                    && build.project_id() == Some(request.project_id)
                    && build.environment_id() == Some(request.environment_id)
                    && build.source_revision_id() == Some(source.id) =>
            {
                build
            }
            Ok(Some(_)) => {
                return Err(ApplicationError::NotFound(
                    "source revision build not found".into(),
                ))
            }
            Ok(None) => {
                return Err(ApplicationError::Conflict(
                    "source revision build is not ready for deployment".into(),
                ))
            }
            Err(error) => return Err(error.into()),
        };
        if build.status != BuildRunStatus::Succeeded {
            return Err(ApplicationError::Conflict(
                "source revision build has not succeeded".into(),
            ));
        }
        let published = match build.published_artifact.as_ref() {
            Some(published) => published,
            None => {
                return Err(ApplicationError::Internal(
                    "successful source revision build omitted its published OCI artifact".into(),
                ))
            }
        };
        SourceBuildAdmission::new(
            ExternalBuildReference {
                organization_id: request.organization_id,
                project_id: request.project_id,
                environment_id: request.environment_id,
                source_revision_id: source.id,
                build_run_id: build.id,
            },
            OciArtifact {
                uri: published.uri.clone(),
                digest: published.digest.clone(),
                media_type: published.media_type.clone(),
            },
        )
        .map_err(ApplicationError::Internal)
    }
}
