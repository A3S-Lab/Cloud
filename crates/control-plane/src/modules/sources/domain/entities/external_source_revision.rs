use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, OrganizationId, ProjectId, SourceRevisionId,
};
use crate::modules::sources::domain::value_objects::{BuildRecipe, GitCommitSha, GitRepository};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalSourceRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub id: SourceRevisionId,
    pub repository: GitRepository,
    pub commit_sha: GitCommitSha,
    pub recipe: BuildRecipe,
    pub recipe_digest: String,
    pub aggregate_version: u64,
    pub accepted_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewExternalSourceRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub id: SourceRevisionId,
    pub repository: GitRepository,
    pub commit_sha: GitCommitSha,
    pub recipe: BuildRecipe,
    pub accepted_at: DateTime<Utc>,
}

impl ExternalSourceRevision {
    pub fn accept(input: NewExternalSourceRevision) -> Result<Self, String> {
        let recipe_digest = input.recipe.digest()?;
        Ok(Self {
            organization_id: input.organization_id,
            project_id: input.project_id,
            environment_id: input.environment_id,
            id: input.id,
            repository: input.repository,
            commit_sha: input.commit_sha,
            recipe: input.recipe,
            recipe_digest,
            aggregate_version: 1,
            accepted_at: canonical_timestamp(input.accepted_at),
        })
    }

    pub fn restore(mut revision: Self) -> Result<Self, String> {
        if revision.aggregate_version != 1 {
            return Err("immutable source revision aggregate version must be 1".into());
        }
        if revision.recipe.digest()? != revision.recipe_digest {
            return Err("source revision recipe digest does not match its recipe".into());
        }
        revision.accepted_at = canonical_timestamp(revision.accepted_at);
        Ok(revision)
    }

    pub fn validate(self) -> Result<Self, String> {
        let repository =
            GitRepository::parse(self.repository.provider(), self.repository.canonical_url())?;
        if repository.identity() != self.repository.identity() {
            return Err("source repository identity does not match its canonical URL".into());
        }
        let commit_sha = GitCommitSha::parse(self.commit_sha.as_str())?;
        let recipe = self.recipe.validate()?;
        Self::restore(Self {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            id: self.id,
            repository,
            commit_sha,
            recipe,
            recipe_digest: self.recipe_digest,
            aggregate_version: self.aggregate_version,
            accepted_at: self.accepted_at,
        })
    }

    pub fn source_identity_digest(&self) -> String {
        self.repository.source_identity_digest(&self.commit_sha)
    }
}
