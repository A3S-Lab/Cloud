use crate::modules::artifacts::domain::{
    BuildRun, BuildRunStatus, BuildSubject, IBuildRunRepository,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, OrganizationId, RepositoryError,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Minimal Artifacts-owned read contract for consumers that must locate a
/// previously published OCI object without loading the BuildRun aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedArtifactLocation {
    build_run_id: BuildRunId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    uri: String,
    digest: String,
    media_type: String,
    size_bytes: u64,
}

impl HostedArtifactLocation {
    fn from_validated_build(build: &BuildRun) -> Result<Option<Self>, String> {
        build.validate()?;
        if build.status != BuildRunStatus::Succeeded {
            return Ok(None);
        }
        let BuildSubject::AssetRelease {
            asset_id,
            asset_release_id,
        } = build.subject
        else {
            return Ok(None);
        };
        let Some(published) = build.published_artifact.as_ref() else {
            return Err("successful BuildRun omitted its OCI publication".into());
        };
        published.validate()?;
        Ok(Some(Self {
            build_run_id: build.id,
            asset_id,
            asset_release_id,
            uri: published.uri.clone(),
            digest: published.digest.clone(),
            media_type: published.media_type.clone(),
            size_bytes: published.size_bytes,
        }))
    }

    pub const fn build_run_id(&self) -> BuildRunId {
        self.build_run_id
    }

    pub const fn asset_id(&self) -> AssetId {
        self.asset_id
    }

    pub const fn asset_release_id(&self) -> AssetReleaseId {
        self.asset_release_id
    }

    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }
}

#[async_trait]
pub trait IHostedArtifactQueryPort: Send + Sync {
    async fn find_hosted_artifact(
        &self,
        organization_id: OrganizationId,
        build_run_id: BuildRunId,
    ) -> Result<Option<HostedArtifactLocation>, RepositoryError>;
}

/// Owner-side adapter that keeps aggregate loading behind the published query
/// port and centralizes the projection for every persistence implementation.
pub struct HostedArtifactQueryService {
    builds: Arc<dyn IBuildRunRepository>,
}

impl HostedArtifactQueryService {
    pub fn new(builds: Arc<dyn IBuildRunRepository>) -> Self {
        Self { builds }
    }
}

#[async_trait]
impl IHostedArtifactQueryPort for HostedArtifactQueryService {
    async fn find_hosted_artifact(
        &self,
        organization_id: OrganizationId,
        build_run_id: BuildRunId,
    ) -> Result<Option<HostedArtifactLocation>, RepositoryError> {
        let build = match self.builds.find(organization_id, build_run_id).await {
            Ok(build) => build,
            Err(RepositoryError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        HostedArtifactLocation::from_validated_build(&build).map_err(|error| {
            RepositoryError::Storage(format!("invalid hosted artifact projection: {error}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::domain::test_support::{
        succeeded_external_build_with_output, succeeded_hosted_build, typed_build_output,
    };
    use crate::modules::artifacts::infrastructure::InMemoryBuildRunRepository;
    use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId, SourceRevisionId};
    use a3s_cloud_contracts::DURABLE_CELL_BUNDLE_MEDIA_TYPE;
    use chrono::{Duration, Utc};

    #[tokio::test]
    async fn hosted_location_retains_its_exact_subject_and_excludes_external_builds() {
        let repository = Arc::new(InMemoryBuildRunRepository::new());
        let organization_id = OrganizationId::new();
        let asset_id = AssetId::new();
        let asset_release_id = AssetReleaseId::new();
        let now = Utc::now();
        let hosted = succeeded_hosted_build(organization_id, asset_id, asset_release_id, now);
        repository.seed_build(hosted.clone()).await;
        let external = succeeded_external_build_with_output(
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            SourceRevisionId::new(),
            typed_build_output(
                &format!("sha256:{}", "f".repeat(64)),
                DURABLE_CELL_BUNDLE_MEDIA_TYPE,
                512,
            ),
            now + Duration::seconds(1),
        );
        repository.seed_build(external.clone()).await;
        let service = HostedArtifactQueryService::new(repository);

        let location = service
            .find_hosted_artifact(organization_id, hosted.id)
            .await
            .expect("hosted location query")
            .expect("hosted location");
        assert_eq!(location.build_run_id(), hosted.id);
        assert_eq!(location.asset_id(), asset_id);
        assert_eq!(location.asset_release_id(), asset_release_id);
        assert_eq!(
            service
                .find_hosted_artifact(organization_id, external.id)
                .await
                .expect("external location query"),
            None
        );
    }
}
