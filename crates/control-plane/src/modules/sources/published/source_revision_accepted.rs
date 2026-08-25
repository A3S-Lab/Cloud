use crate::modules::shared_kernel::domain::{
    EnvironmentId, GitCommitSha, OrganizationId, ProjectId, Sha256Digest, SourceRevisionId,
};
use serde::{Deserialize, Serialize};

pub const SOURCE_REVISION_ACCEPTED_EVENT_KEY: &str = "source.revision.accepted";
pub const SOURCE_REVISION_ACCEPTED_SCHEMA_VERSION: u32 = 1;

/// Immutable Sources fact consumed by contexts that react to an accepted revision.
///
/// The fact intentionally carries no Source aggregate or repository authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRevisionAcceptedFact {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    source_revision_id: SourceRevisionId,
    repository_identity: String,
    commit_sha: String,
    recipe_digest: String,
}

impl SourceRevisionAcceptedFact {
    pub(in crate::modules::sources) fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        source_revision_id: SourceRevisionId,
        repository_identity: String,
        commit_sha: String,
        recipe_digest: String,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            environment_id,
            source_revision_id,
            repository_identity,
            commit_sha,
            recipe_digest,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.source_revision_id.as_uuid().is_nil()
            || self.repository_identity.trim().is_empty()
            || self.repository_identity.len() > 2_048
        {
            return Err("accepted Source revision fact identity is invalid".into());
        }
        GitCommitSha::parse(&self.commit_sha)?;
        Sha256Digest::parse(&self.recipe_digest)?;
        Ok(())
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

    pub fn repository_identity(&self) -> &str {
        &self.repository_identity
    }

    pub fn commit_sha(&self) -> &str {
        &self.commit_sha
    }

    pub fn recipe_digest(&self) -> &str {
        &self.recipe_digest
    }
}
