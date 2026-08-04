use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, OrganizationId, Sha256Digest,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReleaseBinding {
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    build_run_id: BuildRunId,
    artifact_uri: String,
    artifact_digest: Sha256Digest,
    artifact_media_type: String,
    artifact_size_bytes: u64,
}

impl AgentReleaseBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
        build_run_id: BuildRunId,
        artifact_uri: impl Into<String>,
        artifact_digest: Sha256Digest,
        artifact_media_type: impl Into<String>,
        artifact_size_bytes: u64,
    ) -> Result<Self, String> {
        let binding = Self {
            organization_id,
            asset_id,
            asset_release_id,
            build_run_id,
            artifact_uri: artifact_uri.into(),
            artifact_digest,
            artifact_media_type: artifact_media_type.into(),
            artifact_size_bytes,
        };
        binding.validate()?;
        Ok(binding)
    }

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

    pub fn artifact_uri(&self) -> &str {
        &self.artifact_uri
    }

    pub const fn artifact_digest(&self) -> &Sha256Digest {
        &self.artifact_digest
    }

    pub fn artifact_media_type(&self) -> &str {
        &self.artifact_media_type
    }

    pub const fn artifact_size_bytes(&self) -> u64 {
        self.artifact_size_bytes
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.asset_id.as_uuid().is_nil()
            || self.asset_release_id.as_uuid().is_nil()
            || self.build_run_id.as_uuid().is_nil()
            || self.artifact_uri.len() > 2_048
            || !self.artifact_uri.starts_with("oci://")
            || !self
                .artifact_uri
                .ends_with(&format!("@{}", self.artifact_digest.as_str()))
            || self.artifact_media_type.is_empty()
            || self.artifact_media_type.len() > 255
            || self.artifact_media_type.contains(['\0', '\r', '\n'])
            || self.artifact_size_bytes == 0
            || Sha256Digest::parse(self.artifact_digest.as_str())? != self.artifact_digest
        {
            return Err("Agent release binding is invalid".into());
        }
        Ok(())
    }
}
