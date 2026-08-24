use super::{BuildRecipe, GitRepository};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GitCommitSha, OrganizationId, ProjectId, Sha256Digest, SourceRevisionId,
};

/// The minimal immutable Sources-owned input required to start one exact build.
///
/// This is a published snapshot, not a second aggregate. It can only be
/// produced by Sources from a fully validated immutable source revision, and it
/// deliberately omits provider credentials, connection state, timestamps, and
/// aggregate persistence metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBuildInputSnapshot {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    source_revision_id: SourceRevisionId,
    repository: GitRepository,
    commit_sha: GitCommitSha,
    recipe: BuildRecipe,
    recipe_digest: Sha256Digest,
}

pub(in crate::modules::sources) struct ValidatedSourceBuildInputProjection {
    pub(in crate::modules::sources) organization_id: OrganizationId,
    pub(in crate::modules::sources) project_id: ProjectId,
    pub(in crate::modules::sources) environment_id: EnvironmentId,
    pub(in crate::modules::sources) source_revision_id: SourceRevisionId,
    pub(in crate::modules::sources) repository: GitRepository,
    pub(in crate::modules::sources) commit_sha: GitCommitSha,
    pub(in crate::modules::sources) recipe: BuildRecipe,
    pub(in crate::modules::sources) recipe_digest: String,
}

impl SourceBuildInputSnapshot {
    pub const SCHEMA: &'static str = "a3s.cloud.source-build-input.v1";

    pub(in crate::modules::sources) fn from_validated_revision(
        projection: ValidatedSourceBuildInputProjection,
    ) -> Result<Self, String> {
        let recipe_digest = Sha256Digest::parse(projection.recipe_digest)?;
        Ok(Self {
            organization_id: projection.organization_id,
            project_id: projection.project_id,
            environment_id: projection.environment_id,
            source_revision_id: projection.source_revision_id,
            repository: projection.repository,
            commit_sha: projection.commit_sha,
            recipe: projection.recipe,
            recipe_digest,
        })
    }

    pub const fn schema(&self) -> &'static str {
        Self::SCHEMA
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

    pub fn repository(&self) -> &GitRepository {
        &self.repository
    }

    pub fn commit_sha(&self) -> &GitCommitSha {
        &self.commit_sha
    }

    pub fn recipe(&self) -> &BuildRecipe {
        &self.recipe
    }

    pub fn recipe_digest(&self) -> &Sha256Digest {
        &self.recipe_digest
    }
}
