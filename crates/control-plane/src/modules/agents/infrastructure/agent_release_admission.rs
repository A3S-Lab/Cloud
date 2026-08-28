use crate::modules::agents::application::{
    AgentReleaseAdmissionRequest, IAgentReleaseAdmissionPort,
};
use crate::modules::agents::domain::AgentReleaseBinding;
use crate::modules::artifacts::IHostedArtifactQueryPort;
use crate::modules::assets::{load_deployable_agent_release, IAssetRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::Sha256Digest;
use async_trait::async_trait;
use std::sync::Arc;

/// Consumer-owned anti-corruption adapter for the Assets and Artifacts owner
/// interfaces. It creates no repository, cache, lifecycle, or retry mechanism.
#[derive(Clone)]
pub struct AssetsAgentReleaseAdmissionAdapter {
    assets: Arc<dyn IAssetRepository>,
    artifacts: Arc<dyn IHostedArtifactQueryPort>,
}

impl AssetsAgentReleaseAdmissionAdapter {
    pub fn new(
        assets: Arc<dyn IAssetRepository>,
        artifacts: Arc<dyn IHostedArtifactQueryPort>,
    ) -> Self {
        Self { assets, artifacts }
    }
}

#[async_trait]
impl IAgentReleaseAdmissionPort for AssetsAgentReleaseAdmissionAdapter {
    async fn admit(
        &self,
        request: AgentReleaseAdmissionRequest,
    ) -> ApplicationResult<AgentReleaseBinding> {
        let deployable = load_deployable_agent_release(
            self.assets.as_ref(),
            self.artifacts.as_ref(),
            request.organization_id,
            request.asset_id,
            request.asset_release_id,
        )
        .await?;
        AgentReleaseBinding::new(
            deployable.organization_id(),
            deployable.asset_id(),
            deployable.asset_release_id(),
            deployable.build_run_id(),
            deployable.artifact_uri(),
            Sha256Digest::parse(deployable.artifact_digest())
                .map_err(ApplicationError::Internal)?,
            deployable.artifact_media_type(),
            deployable.artifact_size_bytes(),
        )
        .map_err(ApplicationError::Internal)
    }
}
