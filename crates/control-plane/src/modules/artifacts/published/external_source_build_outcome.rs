use crate::modules::shared_kernel::domain::{
    canonical_timestamp, BuildRunId, EnvironmentId, GitCommitSha, OperationId, OrganizationId,
    ProjectId, Sha256Digest, SourceRevisionId,
};
use crate::modules::sources::published::BuildRecipe;
use a3s_cloud_contracts::{OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MANIFEST_MEDIA_TYPE};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const EXTERNAL_SOURCE_BUILD_OUTCOME_SCHEMA: &str = "a3s.cloud.external-source-build-outcome.v1";

/// Immutable OCI publication identity from one successful external-source
/// BuildRun. Node placement, publication credentials, and cleanup state remain
/// private to Artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalSourceBuildArtifact {
    uri: String,
    digest: Sha256Digest,
    media_type: String,
    size_bytes: u64,
}

impl ExternalSourceBuildArtifact {
    fn from_validated_build(
        uri: String,
        digest: String,
        media_type: String,
        size_bytes: u64,
    ) -> Result<Self, String> {
        let artifact = Self {
            uri,
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
        {
            return Err("external source build artifact identity is invalid".into());
        }
        validate_digest_pinned_oci_uri(&self.uri, &self.digest)
    }

    pub fn uri(&self) -> &str {
        &self.uri
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

/// Artifacts-owned fact for a terminal, successful, fully verified external
/// source build. Consumer-specific planning identities are deliberately
/// absent because they remain with their owning context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalSourceBuildOutcome {
    schema: String,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    source_revision_id: SourceRevisionId,
    build_run_id: BuildRunId,
    build_run_version: u64,
    attempt: u32,
    operation_id: OperationId,
    commit_sha: GitCommitSha,
    source_content_digest: Sha256Digest,
    recipe: BuildRecipe,
    artifact: ExternalSourceBuildArtifact,
    provenance_digest: Sha256Digest,
    requested_at: DateTime<Utc>,
    attested_at: DateTime<Utc>,
    completed_at: DateTime<Utc>,
}

pub(in crate::modules::artifacts) struct ValidatedExternalSourceBuildOutcomeProjection {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub source_revision_id: SourceRevisionId,
    pub build_run_id: BuildRunId,
    pub build_run_version: u64,
    pub attempt: u32,
    pub operation_id: OperationId,
    pub commit_sha: String,
    pub source_content_digest: String,
    pub recipe: BuildRecipe,
    pub artifact_uri: String,
    pub artifact_digest: String,
    pub artifact_media_type: String,
    pub artifact_size_bytes: u64,
    pub provenance_digest: String,
    pub requested_at: DateTime<Utc>,
    pub attested_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
}

impl ExternalSourceBuildOutcome {
    pub(in crate::modules::artifacts) fn from_validated_build(
        projection: ValidatedExternalSourceBuildOutcomeProjection,
    ) -> Result<Self, String> {
        let outcome = Self {
            schema: EXTERNAL_SOURCE_BUILD_OUTCOME_SCHEMA.into(),
            organization_id: projection.organization_id,
            project_id: projection.project_id,
            environment_id: projection.environment_id,
            source_revision_id: projection.source_revision_id,
            build_run_id: projection.build_run_id,
            build_run_version: projection.build_run_version,
            attempt: projection.attempt,
            operation_id: projection.operation_id,
            commit_sha: GitCommitSha::parse(projection.commit_sha)?,
            source_content_digest: Sha256Digest::parse(projection.source_content_digest)?,
            recipe: projection.recipe.validate()?,
            artifact: ExternalSourceBuildArtifact::from_validated_build(
                projection.artifact_uri,
                projection.artifact_digest,
                projection.artifact_media_type,
                projection.artifact_size_bytes,
            )?,
            provenance_digest: Sha256Digest::parse(projection.provenance_digest)?,
            requested_at: canonical_timestamp(projection.requested_at),
            attested_at: canonical_timestamp(projection.attested_at),
            completed_at: canonical_timestamp(projection.completed_at),
        };
        outcome.validate()?;
        Ok(outcome)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTERNAL_SOURCE_BUILD_OUTCOME_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.source_revision_id.as_uuid().is_nil()
            || self.build_run_id.as_uuid().is_nil()
            || self.build_run_version == 0
            || self.attempt == 0
            || self.operation_id.as_uuid() != self.build_run_id.as_uuid()
            || GitCommitSha::parse(self.commit_sha.as_str())? != self.commit_sha
            || self.commit_sha.as_str().bytes().all(|byte| byte == b'0')
            || Sha256Digest::parse(self.source_content_digest.as_str())?
                != self.source_content_digest
            || Sha256Digest::parse(self.provenance_digest.as_str())? != self.provenance_digest
            || self.recipe.clone().validate()? != self.recipe
            || self.requested_at != canonical_timestamp(self.requested_at)
            || self.attested_at != canonical_timestamp(self.attested_at)
            || self.completed_at != canonical_timestamp(self.completed_at)
            || self.requested_at > self.attested_at
            || self.attested_at > self.completed_at
        {
            return Err("external source build outcome identity or chronology is invalid".into());
        }
        self.artifact.validate()
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn environment_id(&self) -> EnvironmentId {
        self.environment_id
    }

    pub const fn source_revision_id(&self) -> SourceRevisionId {
        self.source_revision_id
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

    pub const fn source_content_digest(&self) -> &Sha256Digest {
        &self.source_content_digest
    }

    pub const fn recipe(&self) -> &BuildRecipe {
        &self.recipe
    }

    pub const fn artifact(&self) -> &ExternalSourceBuildArtifact {
        &self.artifact
    }

    pub const fn provenance_digest(&self) -> &Sha256Digest {
        &self.provenance_digest
    }

    pub const fn requested_at(&self) -> DateTime<Utc> {
        self.requested_at
    }

    pub const fn attested_at(&self) -> DateTime<Utc> {
        self.attested_at
    }

    pub const fn completed_at(&self) -> DateTime<Utc> {
        self.completed_at
    }
}

fn validate_digest_pinned_oci_uri(uri: &str, digest: &Sha256Digest) -> Result<(), String> {
    let expected_suffix = format!("@{}", digest.as_str());
    let reference = uri
        .strip_prefix("oci://")
        .and_then(|value| value.strip_suffix(&expected_suffix))
        .ok_or_else(|| "external source build artifact is not digest-pinned OCI".to_owned())?;
    let Some((registry, repository)) = reference.split_once('/') else {
        return Err("external source build artifact requires registry and repository".into());
    };
    if uri.len() > 4096
        || registry.is_empty()
        || repository.is_empty()
        || uri.contains(['\0', '\r', '\n', '\t', ' ', '?', '#', '\\'])
        || reference.contains("//")
        || repository.split('/').any(|segment| {
            segment.is_empty()
                || matches!(segment, "." | "..")
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
        || !url::Url::parse(&format!("https://{registry}/")).is_ok_and(|origin| {
            origin.host_str().is_some()
                && origin.path() == "/"
                && origin.query().is_none()
                && origin.fragment().is_none()
                && origin.username().is_empty()
                && origin.password().is_none()
        })
    {
        return Err("external source build artifact OCI reference is invalid".into());
    }
    Ok(())
}
