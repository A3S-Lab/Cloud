use crate::modules::artifacts::application::{
    BuildInputPreparationError, ExternalSourceArchiveRequest, IBuildInputPreparer,
    IExternalSourceArchivePort, INodeArtifactStore, NodeArtifactDescriptor, NodeArtifactStoreError,
    PreparedBuildInput,
};
use crate::modules::artifacts::domain::{
    BuildArtifact, BuildRun, BuildSource, BuildSourceLocation, BuildSubject,
};
use crate::modules::assets::domain::{
    AssetGitBuildInput, AssetGitRepositoryError, IAssetGitRepository, IAssetRepository,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::{artifact_uri, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE};
use a3s_runtime::contract::ArtifactRef;
use async_trait::async_trait;
use std::sync::Arc;

/// Artifacts orchestrator that admits owner-prepared source bytes into the
/// node-artifact store.
///
/// External provider credentials, checkout state, packaging, and local paths
/// remain behind `IExternalSourceArchivePort`. The hosted Asset path remains
/// transitional debt until its owner adapter is extracted independently.
pub struct SourceBuildInputPreparer {
    external_sources: Arc<dyn IExternalSourceArchivePort>,
    assets: Option<Arc<dyn IAssetRepository>>,
    asset_git: Option<Arc<dyn IAssetGitRepository>>,
    artifacts: Arc<dyn INodeArtifactStore>,
}

impl SourceBuildInputPreparer {
    pub fn new(
        external_sources: Arc<dyn IExternalSourceArchivePort>,
        artifacts: Arc<dyn INodeArtifactStore>,
    ) -> Self {
        Self {
            external_sources,
            assets: None,
            asset_git: None,
            artifacts,
        }
    }

    pub fn with_hosted_assets(
        mut self,
        assets: Arc<dyn IAssetRepository>,
        asset_git: Arc<dyn IAssetGitRepository>,
    ) -> Self {
        self.assets = Some(assets);
        self.asset_git = Some(asset_git);
        self
    }

    async fn prepare_external(
        &self,
        build: &BuildRun,
        source: &BuildSource,
    ) -> Result<PreparedBuildInput, BuildInputPreparationError> {
        let BuildSourceLocation::ExternalGit { repository } = &source.location else {
            return Err(BuildInputPreparationError::Conflict);
        };
        let archive = self
            .external_sources
            .prepare(
                ExternalSourceArchiveRequest::new(
                    build.organization_id,
                    build.id,
                    repository.clone(),
                    source.commit_sha.clone(),
                )
                .map_err(BuildInputPreparationError::Invalid)?,
            )
            .await?;
        archive
            .validate()
            .map_err(BuildInputPreparationError::Integrity)?;
        let source_content_digest = archive.source_content_digest().as_str().to_owned();
        let digest = archive.archive_digest().as_str().to_owned();
        let artifact = ArtifactRef {
            uri: artifact_uri(&digest).map_err(BuildInputPreparationError::Invalid)?,
            digest,
            media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
        };
        let descriptor = NodeArtifactDescriptor::new(artifact, archive.size_bytes())
            .map_err(BuildInputPreparationError::Invalid)?;
        let stored = self
            .artifacts
            .put(&descriptor, archive.into_reader())
            .await
            .map_err(map_artifact_error)?;
        if stored.descriptor != descriptor {
            return Err(BuildInputPreparationError::Integrity(
                "node artifact store changed the admitted external Source archive".into(),
            ));
        }
        Ok(PreparedBuildInput {
            source_content_digest,
            artifact: build_artifact(stored.descriptor)?,
        })
    }

    async fn prepare_hosted(
        &self,
        build: &BuildRun,
        source: &BuildSource,
    ) -> Result<PreparedBuildInput, BuildInputPreparationError> {
        let BuildSubject::AssetRelease { asset_id, .. } = build.subject else {
            return Err(BuildInputPreparationError::Conflict);
        };
        if !matches!(
            source.location,
            BuildSourceLocation::HostedAssetGit {
                asset_id: source_asset_id
            } if source_asset_id == asset_id
        ) {
            return Err(BuildInputPreparationError::Conflict);
        }
        let assets = self.assets.as_ref().ok_or_else(|| {
            BuildInputPreparationError::Unavailable(
                "hosted Asset build input adapter is unavailable".into(),
            )
        })?;
        let asset_git = self.asset_git.as_ref().ok_or_else(|| {
            BuildInputPreparationError::Unavailable(
                "hosted Asset Git build input adapter is unavailable".into(),
            )
        })?;
        let asset = assets
            .find_asset(build.organization_id, asset_id)
            .await
            .map_err(map_repository_error)?
            .ok_or_else(|| {
                BuildInputPreparationError::Unavailable("hosted Asset is unavailable".into())
            })?;
        let input = asset_git
            .prepare_build_input(&asset, &source.commit_sha, build.id)
            .await
            .map_err(map_asset_git_error)?;
        let artifact = self.store_hosted_archive(&input).await?;
        Ok(PreparedBuildInput {
            source_content_digest: input.content_digest.as_str().to_owned(),
            artifact,
        })
    }

    async fn store_hosted_archive(
        &self,
        input: &AssetGitBuildInput,
    ) -> Result<BuildArtifact, BuildInputPreparationError> {
        input
            .validate()
            .map_err(BuildInputPreparationError::Integrity)?;
        let digest = input.content_digest.as_str().to_owned();
        let artifact = ArtifactRef {
            uri: artifact_uri(&digest).map_err(BuildInputPreparationError::Invalid)?,
            digest,
            media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
        };
        let descriptor = NodeArtifactDescriptor::new(artifact, input.size_bytes)
            .map_err(BuildInputPreparationError::Invalid)?;
        let file = tokio::fs::File::open(&input.path).await.map_err(|error| {
            BuildInputPreparationError::Storage(format!(
                "could not open hosted Asset build input archive: {error}"
            ))
        })?;
        let stored = self
            .artifacts
            .put(&descriptor, Box::pin(file))
            .await
            .map_err(map_artifact_error)?;
        if stored.descriptor != descriptor {
            return Err(BuildInputPreparationError::Integrity(
                "node artifact store changed the admitted hosted Asset archive".into(),
            ));
        }
        build_artifact(stored.descriptor)
    }
}

#[async_trait]
impl IBuildInputPreparer for SourceBuildInputPreparer {
    async fn prepare(
        &self,
        build: &BuildRun,
        source: &BuildSource,
    ) -> Result<PreparedBuildInput, BuildInputPreparationError> {
        validate_identity(build, source)?;
        match source.location {
            BuildSourceLocation::ExternalGit { .. } => self.prepare_external(build, source).await,
            BuildSourceLocation::HostedAssetGit { .. } => self.prepare_hosted(build, source).await,
        }
    }

    async fn remove(&self, build: &BuildRun) -> Result<(), BuildInputPreparationError> {
        match build.subject {
            BuildSubject::ExternalSourceRevision { .. } => {
                self.external_sources.remove(build.id).await
            }
            BuildSubject::AssetRelease { .. } => {
                let asset_git = self.asset_git.as_ref().ok_or_else(|| {
                    BuildInputPreparationError::Unavailable(
                        "hosted Asset Git build input adapter is unavailable".into(),
                    )
                })?;
                asset_git
                    .remove_build_input(build.id)
                    .await
                    .map_err(map_asset_git_error)
            }
        }
    }
}

fn build_artifact(
    descriptor: NodeArtifactDescriptor,
) -> Result<BuildArtifact, BuildInputPreparationError> {
    BuildArtifact::new(
        descriptor.artifact.uri,
        descriptor.artifact.digest,
        descriptor.artifact.media_type,
        descriptor.size_bytes,
    )
    .map_err(BuildInputPreparationError::Invalid)
}

fn validate_identity(
    build: &BuildRun,
    source: &BuildSource,
) -> Result<(), BuildInputPreparationError> {
    source
        .validate()
        .map_err(BuildInputPreparationError::Integrity)?;
    if build.organization_id != source.organization_id || build.subject != source.subject {
        return Err(BuildInputPreparationError::Conflict);
    }
    Ok(())
}

fn map_repository_error(error: RepositoryError) -> BuildInputPreparationError {
    match error {
        RepositoryError::NotFound => {
            BuildInputPreparationError::Unavailable("hosted Asset is unavailable".into())
        }
        RepositoryError::Conflict(message) => BuildInputPreparationError::Integrity(message),
        RepositoryError::Forbidden(message) => BuildInputPreparationError::Unavailable(message),
        RepositoryError::IdempotencyConflict => BuildInputPreparationError::Conflict,
        RepositoryError::Storage(message) => BuildInputPreparationError::Storage(message),
    }
}

fn map_asset_git_error(error: AssetGitRepositoryError) -> BuildInputPreparationError {
    match error {
        AssetGitRepositoryError::Invalid(message) => BuildInputPreparationError::Invalid(message),
        AssetGitRepositoryError::NotFound => {
            BuildInputPreparationError::Unavailable("hosted Asset source is unavailable".into())
        }
        AssetGitRepositoryError::Integrity(message) => {
            BuildInputPreparationError::Integrity(message)
        }
        AssetGitRepositoryError::QuotaExceeded => {
            BuildInputPreparationError::Invalid("hosted Asset repository quota was exceeded".into())
        }
        AssetGitRepositoryError::BackupUnavailable => {
            BuildInputPreparationError::Unavailable("hosted Asset repository is unavailable".into())
        }
        AssetGitRepositoryError::Storage(message) => BuildInputPreparationError::Storage(message),
    }
}

fn map_artifact_error(error: NodeArtifactStoreError) -> BuildInputPreparationError {
    match error {
        NodeArtifactStoreError::Invalid(message) => BuildInputPreparationError::Invalid(message),
        NodeArtifactStoreError::Conflict => BuildInputPreparationError::Conflict,
        NodeArtifactStoreError::Integrity(message) => {
            BuildInputPreparationError::Integrity(message)
        }
        NodeArtifactStoreError::NotFound => {
            BuildInputPreparationError::Storage("admitted build input disappeared".into())
        }
        NodeArtifactStoreError::Storage(message) => BuildInputPreparationError::Storage(message),
    }
}

#[cfg(test)]
#[path = "source_build_input_preparer_tests.rs"]
mod tests;
