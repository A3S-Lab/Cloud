use crate::modules::shared_kernel::domain::{
    canonical_timestamp, BuildPlanId, BuildRunId, EnvironmentId, GitCommitSha, OrganizationId,
    ProjectId, RepositoryError, Sha256Digest, SourceRevisionId,
};
use crate::modules::sources::published::BuildRecipe;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const WORKLOAD_BUILD_OUTCOME_SCHEMA: &str = "a3s.cloud.developer-workflow-build-outcome.v1";
const OCI_IMAGE_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const OCI_IMAGE_INDEX: &str = "application/vnd.oci.image.index.v1+json";
const DOCKER_IMAGE_MANIFEST: &str = "application/vnd.docker.distribution.manifest.v2+json";
const DOCKER_IMAGE_INDEX: &str = "application/vnd.docker.distribution.manifest.list.v2+json";

/// Minimal artifact evidence required to compile an accepted workload profile.
/// Registry credentials, publication attempts, and retention policy stay with
/// Artifacts and never cross this consumer-owned boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedOciArtifact {
    pub uri: String,
    pub digest: Sha256Digest,
    pub media_type: String,
}

impl VerifiedOciArtifact {
    pub fn validate(&self) -> Result<(), String> {
        if Sha256Digest::parse(self.digest.as_str())? != self.digest {
            return Err("verified workload artifact digest is not canonical".into());
        }
        let expected_suffix = format!("@{}", self.digest.as_str());
        let repository = self
            .uri
            .strip_prefix("oci://")
            .and_then(|value| value.strip_suffix(&expected_suffix))
            .ok_or_else(|| {
                "verified workload artifact is not a digest-pinned OCI reference".to_owned()
            })?;
        if self.uri.len() > 4096
            || !self.uri.starts_with("oci://")
            || !self.uri.ends_with(&expected_suffix)
            || self
                .uri
                .contains(['\0', '\r', '\n', '\t', ' ', '?', '#', '\\'])
            || !matches!(
                self.media_type.as_str(),
                OCI_IMAGE_MANIFEST | OCI_IMAGE_INDEX | DOCKER_IMAGE_MANIFEST | DOCKER_IMAGE_INDEX
            )
        {
            return Err("verified workload artifact is not a digest-pinned OCI reference".into());
        }
        validate_oci_repository(repository)
    }
}

/// Consumer-owned view of an Artifacts-owned, successfully attested build.
///
/// The port implementation is responsible for projecting only terminal,
/// verified outcomes. Developer Workflows validates its exact accepted-plan
/// binding without learning BuildRun states, retries, cleanup commands,
/// publication journals, or aggregate versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedWorkloadBuildOutcome {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub build_plan_id: BuildPlanId,
    pub build_plan_digest: Sha256Digest,
    pub source_revision_id: SourceRevisionId,
    pub build_run_id: BuildRunId,
    pub source_commit_sha: GitCommitSha,
    pub source_content_digest: Sha256Digest,
    pub recipe: BuildRecipe,
    pub artifact: VerifiedOciArtifact,
    pub requested_at: DateTime<Utc>,
    pub attested_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
}

impl VerifiedWorkloadBuildOutcome {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKLOAD_BUILD_OUTCOME_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.build_plan_id.as_uuid().is_nil()
            || self.source_revision_id.as_uuid().is_nil()
            || self.build_run_id.as_uuid().is_nil()
            || self
                .source_commit_sha
                .as_str()
                .bytes()
                .all(|byte| byte == b'0')
            || self.requested_at != canonical_timestamp(self.requested_at)
            || self.attested_at != canonical_timestamp(self.attested_at)
            || self.completed_at != canonical_timestamp(self.completed_at)
            || self.requested_at > self.attested_at
            || self.attested_at > self.completed_at
            || self.recipe.clone().validate()? != self.recipe
        {
            return Err("verified workload build outcome identity or chronology is invalid".into());
        }
        GitCommitSha::parse(self.source_commit_sha.as_str())?;
        Sha256Digest::parse(self.build_plan_digest.as_str())?;
        Sha256Digest::parse(self.source_content_digest.as_str())?;
        self.artifact.validate()
    }
}

fn validate_oci_repository(repository: &str) -> Result<(), String> {
    let Some((registry, path)) = repository.split_once('/') else {
        return Err("verified workload OCI repository requires an explicit registry".into());
    };
    if registry.is_empty()
        || path.is_empty()
        || registry.starts_with('.')
        || registry.ends_with('.')
        || path.starts_with('/')
        || path.ends_with('/')
        || repository.contains("//")
        || repository.split('/').any(|segment| {
            segment.is_empty()
                || segment == "."
                || segment == ".."
                || !segment.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':')
                })
        })
    {
        return Err("verified workload OCI repository is invalid".into());
    }
    Ok(())
}

/// Developer Workflows-owned query port. An Artifacts adapter supplies the
/// immutable outcome; callers never receive the Artifacts aggregate.
#[async_trait]
pub trait IWorkloadBuildOutcomePort: Send + Sync {
    async fn verified_outcome(
        &self,
        organization_id: OrganizationId,
        build_run_id: BuildRunId,
    ) -> Result<Option<VerifiedWorkloadBuildOutcome>, RepositoryError>;
}
