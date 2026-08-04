use crate::modules::artifacts::domain::{BuildRun, IBuildRunRepository};
use crate::modules::assets::domain::{
    Asset, AssetKind, AssetRelease, AssetReleaseState, AssetState, IAssetRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, OrganizationId, RepositoryError,
};
use crate::modules::workloads::domain::entities::OciArtifact;

pub(super) struct DeployableAgentRelease {
    pub asset: Asset,
    pub release: AssetRelease,
    pub build: BuildRun,
    pub artifact: OciArtifact,
}

pub(super) async fn load_deployable_agent_release(
    assets: &dyn IAssetRepository,
    builds: &dyn IBuildRunRepository,
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
) -> ApplicationResult<DeployableAgentRelease> {
    let asset = assets
        .find_asset(organization_id, asset_id)
        .await
        .map_err(ApplicationError::from)?
        .ok_or_else(|| ApplicationError::NotFound("Agent release not found".into()))?;
    let release = assets
        .find_release(organization_id, asset_id, asset_release_id)
        .await
        .map_err(ApplicationError::from)?
        .ok_or_else(|| ApplicationError::NotFound("Agent release not found".into()))?;
    if asset.kind != AssetKind::Agent
        || asset.state != AssetState::Active
        || release.state != AssetReleaseState::Published
    {
        return Err(ApplicationError::Conflict(
            "only a published Agent release can create a new Workload binding".into(),
        ));
    }
    let build_run_id = release
        .provenance
        .as_ref()
        .ok_or_else(|| {
            ApplicationError::Internal(
                "published Agent release omitted its successful BuildRun identity".into(),
            )
        })?
        .build_run_id();
    let build = match builds.find(organization_id, build_run_id).await {
        Ok(build) => build,
        Err(RepositoryError::NotFound) => {
            return Err(ApplicationError::Internal(
                "published Agent release BuildRun is unavailable".into(),
            ))
        }
        Err(error) => return Err(error.into()),
    };
    release
        .validate_build_publication(&asset, &build)
        .map_err(ApplicationError::Internal)?;
    let published = build.published_artifact.as_ref().ok_or_else(|| {
        ApplicationError::Internal(
            "published Agent release BuildRun omitted its OCI publication".into(),
        )
    })?;
    let artifact = OciArtifact {
        uri: published.uri.clone(),
        digest: published.digest.clone(),
        media_type: published.media_type.clone(),
    };
    artifact.validate().map_err(ApplicationError::Internal)?;
    Ok(DeployableAgentRelease {
        asset,
        release,
        build,
        artifact,
    })
}
