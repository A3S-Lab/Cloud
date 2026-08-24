use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, BuildRunId, GitCommitSha, OperationId,
    OrganizationId, Sha256Digest,
};
use a3s_cloud_contracts::{
    MAX_BOX_ARTIFACT_BYTES, OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MANIFEST_MEDIA_TYPE,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const HOSTED_BUILD_OUTCOME_SCHEMA: &str = "a3s.cloud.hosted-build-outcome.v1";
pub const HOSTED_BUILD_OUTCOME_EVENT_KEY: &str = "artifact.hosted-build.succeeded";

/// Immutable, location-free identity of the OCI result admitted by Artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedBuildArtifact {
    digest: Sha256Digest,
    media_type: String,
    size_bytes: u64,
}

impl HostedBuildArtifact {
    fn from_validated_build(
        digest: String,
        media_type: String,
        size_bytes: u64,
    ) -> Result<Self, String> {
        let artifact = Self {
            digest: Sha256Digest::parse(digest)?,
            media_type,
            size_bytes,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if Sha256Digest::parse(self.digest.as_str())? != self.digest
            || !matches!(
                self.media_type.as_str(),
                OCI_IMAGE_INDEX_MEDIA_TYPE | OCI_IMAGE_MANIFEST_MEDIA_TYPE
            )
            || self.size_bytes == 0
            || self.size_bytes > MAX_BOX_ARTIFACT_BYTES
        {
            return Err("hosted build artifact identity, media type, or size is invalid".into());
        }
        Ok(())
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }
}

/// Versioned Artifacts-owned fact used to publish one hosted Asset release.
///
/// Registry locations, node placement, commands, credentials, and the full
/// attestation remain private to Artifacts. Assets receives only the exact
/// immutable evidence required for its own release transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedBuildOutcome {
    schema: String,
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    build_run_id: BuildRunId,
    build_run_version: u64,
    attempt: u32,
    operation_id: OperationId,
    commit_sha: GitCommitSha,
    manifest_digest: Sha256Digest,
    artifact: HostedBuildArtifact,
    provenance_digest: Sha256Digest,
    finished_at: DateTime<Utc>,
}

pub(in crate::modules::artifacts) struct ValidatedHostedBuildOutcomeProjection {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
    pub build_run_id: BuildRunId,
    pub build_run_version: u64,
    pub attempt: u32,
    pub operation_id: OperationId,
    pub commit_sha: String,
    pub manifest_digest: String,
    pub artifact_digest: String,
    pub artifact_media_type: String,
    pub artifact_size_bytes: u64,
    pub provenance_digest: String,
    pub finished_at: DateTime<Utc>,
}

impl HostedBuildOutcome {
    pub(in crate::modules::artifacts) fn from_validated_build(
        projection: ValidatedHostedBuildOutcomeProjection,
    ) -> Result<Self, String> {
        let outcome = Self {
            schema: HOSTED_BUILD_OUTCOME_SCHEMA.into(),
            organization_id: projection.organization_id,
            asset_id: projection.asset_id,
            asset_release_id: projection.asset_release_id,
            build_run_id: projection.build_run_id,
            build_run_version: projection.build_run_version,
            attempt: projection.attempt,
            operation_id: projection.operation_id,
            commit_sha: GitCommitSha::parse(projection.commit_sha)?,
            manifest_digest: Sha256Digest::parse(projection.manifest_digest)?,
            artifact: HostedBuildArtifact::from_validated_build(
                projection.artifact_digest,
                projection.artifact_media_type,
                projection.artifact_size_bytes,
            )?,
            provenance_digest: Sha256Digest::parse(projection.provenance_digest)?,
            finished_at: canonical_timestamp(projection.finished_at),
        };
        outcome.validate()?;
        Ok(outcome)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HOSTED_BUILD_OUTCOME_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.asset_id.as_uuid().is_nil()
            || self.asset_release_id.as_uuid().is_nil()
            || self.build_run_id.as_uuid().is_nil()
            || self.operation_id.as_uuid().is_nil()
            || self.build_run_version == 0
            || self.attempt == 0
            || GitCommitSha::parse(self.commit_sha.as_str())? != self.commit_sha
            || Sha256Digest::parse(self.manifest_digest.as_str())? != self.manifest_digest
            || Sha256Digest::parse(self.provenance_digest.as_str())? != self.provenance_digest
            || self.finished_at != canonical_timestamp(self.finished_at)
        {
            return Err("hosted build outcome identity or evidence is invalid".into());
        }
        self.artifact.validate()
    }

    pub fn schema(&self) -> &str {
        &self.schema
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

    pub const fn build_run_version(&self) -> u64 {
        self.build_run_version
    }

    pub const fn attempt(&self) -> u32 {
        self.attempt
    }

    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    pub const fn commit_sha(&self) -> &GitCommitSha {
        &self.commit_sha
    }

    pub const fn manifest_digest(&self) -> &Sha256Digest {
        &self.manifest_digest
    }

    pub const fn artifact(&self) -> &HostedBuildArtifact {
        &self.artifact
    }

    pub const fn provenance_digest(&self) -> &Sha256Digest {
        &self.provenance_digest
    }

    pub const fn finished_at(&self) -> DateTime<Utc> {
        self.finished_at
    }
}
