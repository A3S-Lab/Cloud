use crate::modules::assets::DeployableAgentRelease;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workloads::domain::entities::{AgentReleaseAdmission, OciArtifact};

/// Translate the Assets application contract into a Workloads-owned value.
/// This is the single anti-corruption boundary between the two contexts.
pub(crate) fn admit_deployable_agent_release(
    release: &DeployableAgentRelease,
) -> ApplicationResult<AgentReleaseAdmission> {
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
    )
    .map_err(ApplicationError::Internal)
}
