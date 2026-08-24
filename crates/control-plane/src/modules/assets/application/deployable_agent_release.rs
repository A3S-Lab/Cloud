use crate::modules::artifacts::application::IHostedArtifactQueryPort;
use crate::modules::assets::domain::{
    AssetKind, AssetReleaseArtifactKind, AssetReleaseState, AssetState, IAssetRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, BuildRunId, OrganizationId};
use chrono::{DateTime, Utc};

/// Assets-owned, immutable read model admitted for an Agent consumer.
///
/// Consumers receive only release identity and OCI coordinates. They never
/// need to load or understand the Artifacts-owned BuildRun aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployableAgentRelease {
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    build_run_id: BuildRunId,
    published_at: DateTime<Utc>,
    artifact_uri: String,
    artifact_digest: String,
    artifact_media_type: String,
    artifact_size_bytes: u64,
}

impl DeployableAgentRelease {
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn asset_id(&self) -> AssetId {
        self.asset_id
    }

    pub const fn asset_release_id(&self) -> AssetReleaseId {
        self.asset_release_id
    }

    pub const fn build_run_id(&self) -> BuildRunId {
        self.build_run_id
    }

    pub const fn published_at(&self) -> DateTime<Utc> {
        self.published_at
    }

    pub fn artifact_uri(&self) -> &str {
        &self.artifact_uri
    }

    pub fn artifact_digest(&self) -> &str {
        &self.artifact_digest
    }

    pub fn artifact_media_type(&self) -> &str {
        &self.artifact_media_type
    }

    pub const fn artifact_size_bytes(&self) -> u64 {
        self.artifact_size_bytes
    }
}

pub async fn load_deployable_agent_release(
    assets: &dyn IAssetRepository,
    artifacts: &dyn IHostedArtifactQueryPort,
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
            "only a published Agent release can create a new binding".into(),
        ));
    }
    release
        .validate_for(&asset)
        .map_err(ApplicationError::Internal)?;
    let provenance = release.provenance.as_ref().ok_or_else(|| {
        ApplicationError::Internal(
            "published Agent release omitted its successful BuildRun identity".into(),
        )
    })?;
    let artifact = release.artifact.as_ref().ok_or_else(|| {
        ApplicationError::Internal("published Agent release omitted its OCI artifact".into())
    })?;
    if artifact.kind() != AssetReleaseArtifactKind::OciService {
        return Err(ApplicationError::Internal(
            "published Agent release did not contain an OCI service artifact".into(),
        ));
    }
    let published_at = release.published_at.ok_or_else(|| {
        ApplicationError::Internal("published Agent release omitted its publication time".into())
    })?;
    let location = artifacts
        .find_hosted_artifact(organization_id, provenance.build_run_id())
        .await
        .map_err(ApplicationError::from)?
        .ok_or_else(|| {
            ApplicationError::Internal("published Agent release OCI location is unavailable".into())
        })?;
    if location.build_run_id() != provenance.build_run_id()
        || location.asset_id() != asset_id
        || location.asset_release_id() != asset_release_id
        || location.digest() != artifact.digest().as_str()
        || location.media_type() != artifact.media_type()
        || location.size_bytes() != artifact.size_bytes()
    {
        return Err(ApplicationError::Internal(
            "published Agent release changed its OCI artifact location identity".into(),
        ));
    }
    Ok(DeployableAgentRelease {
        organization_id,
        asset_id,
        asset_release_id,
        build_run_id: provenance.build_run_id(),
        published_at,
        artifact_uri: location.uri().into(),
        artifact_digest: artifact.digest().to_string(),
        artifact_media_type: artifact.media_type().into(),
        artifact_size_bytes: artifact.size_bytes(),
    })
}
