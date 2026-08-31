use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, BuildRunId, GitCommitSha, OperationId,
    OrganizationId, Sha256Digest,
};
use a3s_cloud_contracts::{
    agent_harness_compatibility_v1, agent_release_builder_uri, agent_release_manifest_archive,
    agent_release_source_uri, AgentReleaseManifest, MAX_BOX_ARTIFACT_BYTES,
    OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MANIFEST_MEDIA_TYPE,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const HOSTED_BUILD_OUTCOME_SCHEMA: &str = "a3s.cloud.hosted-build-outcome.v2";
pub const LEGACY_HOSTED_BUILD_OUTCOME_SCHEMA: &str = "a3s.cloud.hosted-build-outcome.v1";
pub const HOSTED_BUILD_OUTCOME_EVENT_KEY: &str = "artifact.hosted-build.succeeded";

/// Immutable, location-free identity of the OCI result admitted by Artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedBuildArtifact {
    digest: Sha256Digest,
    media_type: String,
    size_bytes: u64,
}

/// Exact final Code manifest and deterministic directory archive published by
/// one hosted Agent build. The archive URI is derived from its digest by the
/// node artifact protocol and is therefore not duplicated in this owner fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedAgentReleaseManifest {
    identity: Sha256Digest,
    canonical_acl: String,
    archive_digest: Sha256Digest,
    archive_size_bytes: u64,
}

impl HostedAgentReleaseManifest {
    pub(in crate::modules::artifacts) fn from_validated_parts(
        identity: String,
        canonical_acl: String,
        archive_digest: String,
        archive_size_bytes: u64,
    ) -> Result<Self, String> {
        let value = Self {
            identity: Sha256Digest::parse(identity)?,
            canonical_acl,
            archive_digest: Sha256Digest::parse(archive_digest)?,
            archive_size_bytes,
        };
        value.validate_shape()?;
        Ok(value)
    }

    fn validate_shape(&self) -> Result<AgentReleaseManifest, String> {
        let manifest = AgentReleaseManifest::parse(&self.canonical_acl)
            .map_err(|error| format!("hosted Agent release manifest is invalid: {error}"))?;
        manifest
            .verify_compatibility(&agent_harness_compatibility_v1())
            .map_err(|error| format!("hosted Agent release manifest is incompatible: {error}"))?;
        let archive = agent_release_manifest_archive(self.canonical_acl.as_bytes())?;
        if manifest.canonical_acl() != self.canonical_acl
            || manifest.identity() != self.identity.as_str()
            || Sha256Digest::from_bytes(&archive) != self.archive_digest
            || archive.len() as u64 != self.archive_size_bytes
        {
            return Err("hosted Agent release manifest changed its exact bytes".into());
        }
        Ok(manifest)
    }

    fn validate_for(&self, outcome: &HostedBuildOutcome) -> Result<(), String> {
        let manifest = self.validate_shape()?;
        let source_content_digest = outcome.source_content_digest.as_ref().ok_or_else(|| {
            "hosted Agent release manifest omitted its source content digest".to_owned()
        })?;
        let source_uri = agent_release_source_uri(source_content_digest.as_str())?;
        let builder_uri = agent_release_builder_uri(outcome.build_run_id.as_uuid())?;
        if manifest.artifact().digest() != outcome.artifact.digest().as_str()
            || manifest.artifact().media_type() != outcome.artifact.media_type()
            || manifest.provenance().len() != 2
            || !manifest.provenance().iter().any(|reference| {
                reference.kind() == "source"
                    && reference.uri() == source_uri
                    && reference.digest() == source_content_digest.as_str()
            })
            || !manifest.provenance().iter().any(|reference| {
                reference.kind() == "builder"
                    && reference.uri() == builder_uri
                    && reference.digest() == outcome.provenance_digest.as_str()
            })
        {
            return Err("hosted Agent release manifest changed its build bindings".into());
        }
        Ok(())
    }

    pub const fn identity(&self) -> &Sha256Digest {
        &self.identity
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn archive_digest(&self) -> &Sha256Digest {
        &self.archive_digest
    }

    pub const fn archive_size_bytes(&self) -> u64 {
        self.archive_size_bytes
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_content_digest: Option<Sha256Digest>,
    artifact: HostedBuildArtifact,
    provenance_digest: Sha256Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent_release_manifest: Option<HostedAgentReleaseManifest>,
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
    pub source_content_digest: String,
    pub artifact_digest: String,
    pub artifact_media_type: String,
    pub artifact_size_bytes: u64,
    pub provenance_digest: String,
    pub agent_release_manifest: Option<HostedAgentReleaseManifest>,
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
            source_content_digest: Some(Sha256Digest::parse(projection.source_content_digest)?),
            artifact: HostedBuildArtifact::from_validated_build(
                projection.artifact_digest,
                projection.artifact_media_type,
                projection.artifact_size_bytes,
            )?,
            provenance_digest: Sha256Digest::parse(projection.provenance_digest)?,
            agent_release_manifest: projection.agent_release_manifest,
            finished_at: canonical_timestamp(projection.finished_at),
        };
        outcome.validate()?;
        Ok(outcome)
    }

    pub fn validate(&self) -> Result<(), String> {
        let is_current =
            self.schema == HOSTED_BUILD_OUTCOME_SCHEMA && self.source_content_digest.is_some();
        let is_legacy = self.schema == LEGACY_HOSTED_BUILD_OUTCOME_SCHEMA
            && self.source_content_digest.is_none()
            && self.agent_release_manifest.is_none();
        if (!is_current && !is_legacy)
            || self.organization_id.as_uuid().is_nil()
            || self.asset_id.as_uuid().is_nil()
            || self.asset_release_id.as_uuid().is_nil()
            || self.build_run_id.as_uuid().is_nil()
            || self.operation_id.as_uuid().is_nil()
            || self.build_run_version == 0
            || self.attempt == 0
            || GitCommitSha::parse(self.commit_sha.as_str())? != self.commit_sha
            || Sha256Digest::parse(self.manifest_digest.as_str())? != self.manifest_digest
            || self.source_content_digest.as_ref().is_some_and(|digest| {
                match Sha256Digest::parse(digest.as_str()) {
                    Ok(canonical) => canonical != *digest,
                    Err(_) => true,
                }
            })
            || Sha256Digest::parse(self.provenance_digest.as_str())? != self.provenance_digest
            || self.finished_at != canonical_timestamp(self.finished_at)
        {
            return Err("hosted build outcome identity or evidence is invalid".into());
        }
        self.artifact.validate()?;
        if let Some(manifest) = &self.agent_release_manifest {
            manifest.validate_for(self)?;
        }
        Ok(())
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

    pub const fn source_content_digest(&self) -> Option<&Sha256Digest> {
        self.source_content_digest.as_ref()
    }

    pub const fn provenance_digest(&self) -> &Sha256Digest {
        &self.provenance_digest
    }

    pub const fn agent_release_manifest(&self) -> Option<&HostedAgentReleaseManifest> {
        self.agent_release_manifest.as_ref()
    }

    pub fn is_legacy(&self) -> bool {
        self.schema == LEGACY_HOSTED_BUILD_OUTCOME_SCHEMA
    }

    pub const fn finished_at(&self) -> DateTime<Utc> {
        self.finished_at
    }
}
