use super::GetBuildEvidence;
use crate::modules::artifacts::application::resource_access::BuildRunResourceAccess;
use crate::modules::artifacts::domain::{BuildEvidence, IBuildRunRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
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
            let build_run = match BuildRunResourceAccess::new(builds)
                .build_run(
                    query.organization_id,
                    query.build_run_id,
                    &query.access,
                    "build evidence not found",
                )
                .await
            {
                Ok(build_run) => build_run,
                Err(error) => return Ok(Err(error)),
            };
            Ok(build_run
                .evidence
                .map(|evidence| *evidence)
                .ok_or_else(evidence_not_found))
        })
    }
}

fn evidence_not_found() -> ApplicationError {
    ApplicationError::NotFound("build evidence not found".into())
}
