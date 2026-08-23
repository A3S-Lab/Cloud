use crate::modules::developer_workflows::domain::AcceptedBuildPlan;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GitCommitSha, OrganizationId, ProjectId, RepositoryError,
    Sha256Digest, SourceRevisionId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlanSourceRevisionEvidence {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub source_revision_id: SourceRevisionId,
    pub source_identity_digest: Sha256Digest,
    pub commit_sha: GitCommitSha,
    pub recipe_digest: Sha256Digest,
    pub accepted_at: DateTime<Utc>,
}

impl BuildPlanSourceRevisionEvidence {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.source_revision_id.as_uuid().is_nil()
            || self.accepted_at != canonical_timestamp(self.accepted_at)
            || Sha256Digest::parse(self.source_identity_digest.as_str())?
                != self.source_identity_digest
            || GitCommitSha::parse(self.commit_sha.as_str())? != self.commit_sha
            || Sha256Digest::parse(self.recipe_digest.as_str())? != self.recipe_digest
        {
            return Err("BuildPlan Source revision evidence is invalid".into());
        }
        Ok(())
    }

    pub fn validate_binding(&self, plan: &AcceptedBuildPlan) -> Result<(), String> {
        self.validate()?;
        plan.validate()?;
        let proposal = &plan.contract.spec().proposal;
        let spec = proposal.spec();
        let recipe_digest = Sha256Digest::parse(spec.recipe.digest()?)?;
        if self.organization_id != plan.organization_id
            || self.project_id != plan.project_id
            || self.environment_id != plan.environment_id
            || self.source_revision_id != plan.source_revision_id
            || self.source_identity_digest != spec.source.source_identity_digest
            || self.commit_sha != spec.source.commit_sha
            || self.recipe_digest != recipe_digest
            || plan.accepted_at < self.accepted_at
        {
            return Err("accepted BuildPlan does not match its exact Source revision".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait IBuildPlanSourceRevisionPort: Send + Sync {
    async fn resolve(
        &self,
        organization_id: OrganizationId,
        source_revision_id: SourceRevisionId,
    ) -> Result<Option<BuildPlanSourceRevisionEvidence>, RepositoryError>;
}
