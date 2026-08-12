use super::GetBuildRun;
use crate::modules::artifacts::application::resource_access::BuildRunResourceAccess;
use crate::modules::artifacts::domain::{BuildRun, IBuildRunRepository};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetBuildRunHandler {
    builds: Arc<dyn IBuildRunRepository>,
}

impl GetBuildRunHandler {
    pub fn new(builds: Arc<dyn IBuildRunRepository>) -> Self {
        Self { builds }
    }
}

impl QueryHandler<GetBuildRun> for GetBuildRunHandler {
    fn execute(
        &self,
        query: GetBuildRun,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<BuildRun>>> {
        let builds = Arc::clone(&self.builds);
        Box::pin(async move {
            Ok(BuildRunResourceAccess::new(builds)
                .build_run(
                    query.organization_id,
                    query.build_run_id,
                    &query.resource_access,
                    "build run not found",
                )
                .await)
        })
    }
}
