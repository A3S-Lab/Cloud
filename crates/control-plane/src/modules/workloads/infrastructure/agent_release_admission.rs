use crate::modules::artifacts::IHostedArtifactQueryPort;
use crate::modules::assets::{
    load_deployable_agent_release, DeployableAgentRelease, IAssetRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workloads::application::{
    IWorkloadAgentReleaseAdmissionPort, WorkloadAgentReleaseAdmissionRequest,
};
use crate::modules::workloads::domain::entities::{
    AgentReleaseAdmission, AgentReleaseRuntimeContract, OciArtifact,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Consumer-owned anti-corruption adapter for the Assets and Artifacts owner
/// interfaces. It creates no repository, cache, lifecycle, or retry mechanism.
#[derive(Clone)]
pub struct AssetsWorkloadAgentReleaseAdmissionAdapter {
    assets: Arc<dyn IAssetRepository>,
    artifacts: Arc<dyn IHostedArtifactQueryPort>,
}

impl AssetsWorkloadAgentReleaseAdmissionAdapter {
    pub fn new(
        assets: Arc<dyn IAssetRepository>,
        artifacts: Arc<dyn IHostedArtifactQueryPort>,
    ) -> Self {
        Self { assets, artifacts }
    }
}

#[async_trait]
impl IWorkloadAgentReleaseAdmissionPort for AssetsWorkloadAgentReleaseAdmissionAdapter {
    async fn admit(
        &self,
        request: WorkloadAgentReleaseAdmissionRequest,
    ) -> ApplicationResult<AgentReleaseAdmission> {
        let deployable = load_deployable_agent_release(
            self.assets.as_ref(),
            self.artifacts.as_ref(),
            request.organization_id,
            request.asset_id,
            request.asset_release_id,
        )
        .await?;
        admit_deployable_agent_release(&deployable)
    }
}

/// Translate the Assets application contract into a Workloads-owned value.
/// This is the single anti-corruption boundary between the two contexts.
fn admit_deployable_agent_release(
    release: &DeployableAgentRelease,
) -> ApplicationResult<AgentReleaseAdmission> {
    let runtime_contract = AgentReleaseRuntimeContract::new(
        release.manifest_identity(),
        release.manifest_acl(),
        release.manifest_artifact_uri(),
        release.manifest_artifact_digest(),
        release.manifest_artifact_size_bytes(),
    )
    .map_err(ApplicationError::Internal)?;
    AgentReleaseAdmission::new(
        release.organization_id(),
        release.asset_id(),
        release.asset_release_id(),
        release.build_run_id(),
        release.published_at(),
        OciArtifact {
            uri: release.artifact_uri().into(),
            digest: release.artifact_digest().into(),
            media_type: release.artifact_media_type().into(),
        },
        runtime_contract,
    )
    .map_err(ApplicationError::Internal)
}
