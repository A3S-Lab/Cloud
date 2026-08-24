use crate::modules::artifacts::domain::{
    BuildRun, BuildSource, BuildSourceResolutionError, BuildSubject, IBuildSourceResolver,
};
use crate::modules::assets::{HostedAssetBuildInputQueryError, IHostedAssetBuildInputQueryPort};
use crate::modules::sources::{ISourceBuildInputQueryPort, SourceBuildInputQueryError};
use async_trait::async_trait;
use std::sync::Arc;

pub struct CloudBuildSourceResolver {
    sources: Arc<dyn ISourceBuildInputQueryPort>,
    hosted_assets: Arc<dyn IHostedAssetBuildInputQueryPort>,
}

impl CloudBuildSourceResolver {
    pub fn new(
        sources: Arc<dyn ISourceBuildInputQueryPort>,
        hosted_assets: Arc<dyn IHostedAssetBuildInputQueryPort>,
    ) -> Self {
        Self {
            sources,
            hosted_assets,
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
                let input = self
                    .sources
                    .find_source_build_input(
                        build.organization_id,
                        project_id,
                        environment_id,
                        source_revision_id,
                    )
                    .await
                    .map_err(map_source_query_error)?
                    .ok_or(BuildSourceResolutionError::NotFound)?;
                if input.organization_id() != build.organization_id
                    || input.project_id() != project_id
                    || input.environment_id() != environment_id
                    || input.source_revision_id() != source_revision_id
                {
                    return Err(BuildSourceResolutionError::Conflict);
                }
                BuildSource::from_source_input(&input)
                    .map_err(BuildSourceResolutionError::Integrity)
            }
            BuildSubject::AssetRelease {
                asset_id,
                asset_release_id,
            } => {
                let input = self
                    .hosted_assets
                    .find_hosted_asset_build_input(
                        build.organization_id,
                        asset_id,
                        asset_release_id,
                    )
                    .await
                    .map_err(map_hosted_asset_query_error)?
                    .ok_or(BuildSourceResolutionError::NotFound)?;
                if input.organization_id() != build.organization_id
                    || input.asset_id() != asset_id
                    || input.asset_release_id() != asset_release_id
                {
                    return Err(BuildSourceResolutionError::Conflict);
                }
                BuildSource::hosted_asset(
                    build.organization_id,
                    build.subject,
                    input.commit_sha().clone(),
                    input.manifest_digest().clone(),
                    input.recipe().clone(),
                )
                .map_err(BuildSourceResolutionError::Integrity)
            }
        }
    }
}

fn map_source_query_error(error: SourceBuildInputQueryError) -> BuildSourceResolutionError {
    match error {
        SourceBuildInputQueryError::Invalid(message) => {
            BuildSourceResolutionError::Invalid(message)
        }
        SourceBuildInputQueryError::Conflict => BuildSourceResolutionError::Conflict,
        SourceBuildInputQueryError::Integrity(message) => {
            BuildSourceResolutionError::Integrity(message)
        }
        SourceBuildInputQueryError::Storage(message) => {
            BuildSourceResolutionError::Storage(message)
        }
    }
}

fn map_hosted_asset_query_error(
    error: HostedAssetBuildInputQueryError,
) -> BuildSourceResolutionError {
    match error {
        HostedAssetBuildInputQueryError::Invalid(message) => {
            BuildSourceResolutionError::Invalid(message)
        }
        HostedAssetBuildInputQueryError::Conflict => BuildSourceResolutionError::Conflict,
        HostedAssetBuildInputQueryError::NotFound => BuildSourceResolutionError::NotFound,
        HostedAssetBuildInputQueryError::Unavailable(message) => {
            BuildSourceResolutionError::Unavailable(message)
        }
        HostedAssetBuildInputQueryError::Integrity(message) => {
            BuildSourceResolutionError::Integrity(message)
        }
        HostedAssetBuildInputQueryError::Storage(message) => {
            BuildSourceResolutionError::Storage(message)
        }
    }
}
