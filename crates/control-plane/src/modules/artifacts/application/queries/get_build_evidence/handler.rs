use super::GetBuildEvidence;
use crate::modules::artifacts::domain::{BuildEvidence, IBuildRunRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetBuildEvidenceHandler {
    builds: Arc<dyn IBuildRunRepository>,
}

impl GetBuildEvidenceHandler {
    pub fn new(builds: Arc<dyn IBuildRunRepository>) -> Self {
        Self { builds }
    }
}

impl QueryHandler<GetBuildEvidence> for GetBuildEvidenceHandler {
    fn execute(
        &self,
        query: GetBuildEvidence,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<BuildEvidence>>> {
        let builds = Arc::clone(&self.builds);
        Box::pin(async move {
            Ok(
                match builds.find(query.organization_id, query.build_run_id).await {
                    Ok(build_run) => build_run
                        .evidence
                        .map(|evidence| *evidence)
                        .ok_or_else(evidence_not_found),
                    Err(RepositoryError::NotFound) => Err(evidence_not_found()),
                    Err(error) => Err(error.into()),
                },
            )
        })
    }
}

fn evidence_not_found() -> ApplicationError {
    ApplicationError::NotFound("build evidence not found".into())
}
