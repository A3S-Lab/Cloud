use crate::modules::assets::domain::{
    Asset, AssetKind, AssetRelease, AssetReleaseArtifactKind, AssetReleaseState, AssetState,
    IAssetRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, OrganizationId};

pub(super) struct DeployableSkillRelease {
    pub asset: Asset,
    pub release: AssetRelease,
}

pub(super) async fn load_deployable_skill_release(
    assets: &dyn IAssetRepository,
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
) -> ApplicationResult<DeployableSkillRelease> {
    let asset = assets
        .find_asset(organization_id, asset_id)
        .await
        .map_err(ApplicationError::from)?
        .ok_or_else(|| ApplicationError::NotFound("Skill release not found".into()))?;
    let release = assets
        .find_release(organization_id, asset_id, asset_release_id)
        .await
        .map_err(ApplicationError::from)?
        .ok_or_else(|| ApplicationError::NotFound("Skill release not found".into()))?;
    if asset.kind != AssetKind::Skill
        || asset.state != AssetState::Active
        || release.state != AssetReleaseState::Published
        || !release.artifact.as_ref().is_some_and(|artifact| {
            artifact.kind() == AssetReleaseArtifactKind::SkillBundle
                && artifact.media_type() == a3s_cloud_contracts::SKILL_BUNDLE_MEDIA_TYPE
        })
    {
        return Err(ApplicationError::Conflict(
            "only a published Skill bundle can create a new Workload binding".into(),
        ));
    }
    release
        .validate_for(&asset)
        .map_err(ApplicationError::Internal)?;
    Ok(DeployableSkillRelease { asset, release })
}
