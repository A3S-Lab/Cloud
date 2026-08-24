use crate::modules::artifacts::domain::{
    BuildRun, BuildSource, BuildSourceResolutionError, BuildSubject, IBuildSourceResolver,
};
use crate::modules::assets::domain::{
    AssetGitRepositoryError, AssetKind, IAssetGitRepository, IAssetRepository,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::sources::domain::ISourceRevisionRepository;
use crate::modules::sources::publish_source_build_input;
use async_trait::async_trait;
use std::sync::Arc;

pub struct CloudBuildSourceResolver {
    sources: Arc<dyn ISourceRevisionRepository>,
    assets: Arc<dyn IAssetRepository>,
    asset_git: Arc<dyn IAssetGitRepository>,
}

impl CloudBuildSourceResolver {
    pub fn new(
        sources: Arc<dyn ISourceRevisionRepository>,
        assets: Arc<dyn IAssetRepository>,
        asset_git: Arc<dyn IAssetGitRepository>,
    ) -> Self {
        Self {
            sources,
            assets,
            asset_git,
        }
    }
}

#[async_trait]
impl IBuildSourceResolver for CloudBuildSourceResolver {
    async fn resolve(&self, build: &BuildRun) -> Result<BuildSource, BuildSourceResolutionError> {
        build
            .clone()
            .restore()
            .map_err(BuildSourceResolutionError::Invalid)?;
        match build.subject {
            BuildSubject::ExternalSourceRevision {
                project_id,
                environment_id,
                source_revision_id,
            } => {
                let revision = self
                    .sources
                    .find(build.organization_id, source_revision_id)
                    .await
                    .map_err(map_repository_error)?;
                if revision.organization_id != build.organization_id
                    || revision.project_id != project_id
                    || revision.environment_id != environment_id
                    || revision.id != source_revision_id
                {
                    return Err(BuildSourceResolutionError::Conflict);
                }
                let input = publish_source_build_input(&revision)
                    .map_err(BuildSourceResolutionError::Integrity)?;
                BuildSource::from_source_input(&input)
                    .map_err(BuildSourceResolutionError::Integrity)
            }
            BuildSubject::AssetRelease {
                asset_id,
                asset_release_id,
            } => {
                let asset = self
                    .assets
                    .find_asset(build.organization_id, asset_id)
                    .await
                    .map_err(map_repository_error)?
                    .ok_or(BuildSourceResolutionError::NotFound)?;
                let release = self
                    .assets
                    .find_release(build.organization_id, asset_id, asset_release_id)
                    .await
                    .map_err(map_repository_error)?
                    .ok_or(BuildSourceResolutionError::NotFound)?;
                release
                    .validate_for(&asset)
                    .map_err(BuildSourceResolutionError::Integrity)?;
                if asset.kind == AssetKind::Skill {
                    return Err(BuildSourceResolutionError::Invalid(
                        "Skill bundle publication is owned by A0.5 and cannot use the OCI build output contract"
                            .into(),
                    ));
                }
                let admission = self
                    .asset_git
                    .admit_manifest(&asset, &release.commit_sha)
                    .await
                    .map_err(map_asset_git_error)?;
                admission
                    .validate_for(asset.kind)
                    .map_err(BuildSourceResolutionError::Integrity)?;
                if admission.commit_sha != release.commit_sha
                    || admission.manifest_digest != release.manifest_digest
                {
                    return Err(BuildSourceResolutionError::Integrity(
                        "pinned Asset manifest changed after release draft creation".into(),
                    ));
                }
                let recipe = admission.build_recipe.ok_or_else(|| {
                    BuildSourceResolutionError::Invalid(
                        "Agent and MCP release publication requires one pinned Asset build block"
                            .into(),
                    )
                })?;
                BuildSource::hosted_asset(
                    build.organization_id,
                    build.subject,
                    release.commit_sha,
                    release.manifest_digest,
                    recipe,
                )
                .map_err(BuildSourceResolutionError::Integrity)
            }
        }
    }
}

fn map_repository_error(error: RepositoryError) -> BuildSourceResolutionError {
    match error {
        RepositoryError::NotFound => BuildSourceResolutionError::NotFound,
        RepositoryError::Conflict(_) => BuildSourceResolutionError::Conflict,
        RepositoryError::Forbidden(_) => BuildSourceResolutionError::Conflict,
        RepositoryError::IdempotencyConflict => BuildSourceResolutionError::Conflict,
        RepositoryError::Storage(message) => BuildSourceResolutionError::Storage(message),
    }
}

fn map_asset_git_error(error: AssetGitRepositoryError) -> BuildSourceResolutionError {
    match error {
        AssetGitRepositoryError::Invalid(message) => BuildSourceResolutionError::Invalid(message),
        AssetGitRepositoryError::NotFound => BuildSourceResolutionError::NotFound,
        AssetGitRepositoryError::Integrity(message) => {
            BuildSourceResolutionError::Integrity(message)
        }
        AssetGitRepositoryError::QuotaExceeded => {
            BuildSourceResolutionError::Invalid("hosted Git repository quota was exceeded".into())
        }
        AssetGitRepositoryError::BackupUnavailable => {
            BuildSourceResolutionError::Unavailable("hosted Git repository is unavailable".into())
        }
        AssetGitRepositoryError::Storage(message) => BuildSourceResolutionError::Storage(message),
    }
}
