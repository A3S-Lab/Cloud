use crate::modules::assets::domain::{
    AssetKind, AssetReleaseArtifactKind, AssetReleaseState, AssetState,
};
use crate::modules::assets::IAssetRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workloads::application::{
    IWorkloadSkillReleaseAdmissionPort, WorkloadSkillReleaseAdmissionRequest,
};
use crate::modules::workloads::domain::entities::SkillReleaseAdmission;
use a3s_cloud_contracts::SKILL_BUNDLE_MEDIA_TYPE;
use async_trait::async_trait;
use std::sync::Arc;

/// Consumer-owned anti-corruption adapter for the Assets owner interface.
/// It creates no repository, cache, lifecycle, or retry mechanism.
#[derive(Clone)]
pub struct AssetsWorkloadSkillReleaseAdmissionAdapter {
    assets: Arc<dyn IAssetRepository>,
}

impl AssetsWorkloadSkillReleaseAdmissionAdapter {
    pub fn new(assets: Arc<dyn IAssetRepository>) -> Self {
        Self { assets }
    }
}

#[async_trait]
impl IWorkloadSkillReleaseAdmissionPort for AssetsWorkloadSkillReleaseAdmissionAdapter {
    async fn admit(
        &self,
        request: WorkloadSkillReleaseAdmissionRequest,
    ) -> ApplicationResult<SkillReleaseAdmission> {
        let asset = self
            .assets
            .find_asset(request.organization_id, request.asset_id)
            .await
            .map_err(ApplicationError::from)?
            .ok_or_else(|| ApplicationError::NotFound("Skill release not found".into()))?;
        let release = self
            .assets
            .find_release(
                request.organization_id,
                request.asset_id,
                request.asset_release_id,
            )
            .await
            .map_err(ApplicationError::from)?
            .ok_or_else(|| ApplicationError::NotFound("Skill release not found".into()))?;
        if asset.kind != AssetKind::Skill
            || asset.state != AssetState::Active
            || release.state != AssetReleaseState::Published
            || !release.artifact.as_ref().is_some_and(|artifact| {
                artifact.kind() == AssetReleaseArtifactKind::SkillBundle
                    && artifact.media_type() == SKILL_BUNDLE_MEDIA_TYPE
            })
        {
            return Err(ApplicationError::Conflict(
                "only a published Skill bundle can create a new Workload binding".into(),
            ));
        }
        release
            .validate_for(&asset)
            .map_err(ApplicationError::Internal)?;
        let artifact = release.artifact.as_ref().ok_or_else(|| {
            ApplicationError::Internal("published Skill release omitted its bundle artifact".into())
        })?;
        SkillReleaseAdmission::new(
            asset.organization_id,
            asset.id,
            release.id,
            release.updated_at,
            artifact.digest().clone(),
            artifact.size_bytes(),
        )
        .map_err(ApplicationError::Internal)
    }
}
