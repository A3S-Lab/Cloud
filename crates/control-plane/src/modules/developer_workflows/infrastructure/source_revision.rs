use crate::modules::developer_workflows::application::{
    BuildPlanSourceRevisionEvidence, IBuildPlanSourceRevisionPort,
};
use crate::modules::shared_kernel::domain::{
    OrganizationId, RepositoryError, Sha256Digest, SourceRevisionId,
};
use crate::modules::sources::domain::ISourceRevisionRepository;
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Clone)]
pub struct RepositoryBuildPlanSourceRevisionPort {
    sources: Arc<dyn ISourceRevisionRepository>,
}

impl RepositoryBuildPlanSourceRevisionPort {
    pub fn new(sources: Arc<dyn ISourceRevisionRepository>) -> Self {
        Self { sources }
    }
}

#[async_trait]
impl IBuildPlanSourceRevisionPort for RepositoryBuildPlanSourceRevisionPort {
    async fn resolve(
        &self,
        organization_id: OrganizationId,
        source_revision_id: SourceRevisionId,
    ) -> Result<Option<BuildPlanSourceRevisionEvidence>, RepositoryError> {
        let revision = match self.sources.find(organization_id, source_revision_id).await {
            Ok(value) => value,
            Err(RepositoryError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        let evidence = BuildPlanSourceRevisionEvidence {
            organization_id: revision.organization_id,
            project_id: revision.project_id,
            environment_id: revision.environment_id,
            source_revision_id: revision.id,
            source_identity_digest: Sha256Digest::parse(revision.source_identity_digest())
                .map_err(RepositoryError::Storage)?,
            commit_sha: revision.commit_sha,
            recipe_digest: Sha256Digest::parse(revision.recipe_digest)
                .map_err(RepositoryError::Storage)?,
            accepted_at: revision.accepted_at,
        };
        evidence.validate().map_err(RepositoryError::Storage)?;
        Ok(Some(evidence))
    }
}
