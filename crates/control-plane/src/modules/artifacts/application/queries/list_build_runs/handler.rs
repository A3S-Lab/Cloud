use super::ListBuildRuns;
use crate::modules::artifacts::domain::{BuildRun, IBuildRunRepository};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListBuildRunsHandler {
    builds: Arc<dyn IBuildRunRepository>,
}

impl ListBuildRunsHandler {
    pub fn new(builds: Arc<dyn IBuildRunRepository>) -> Self {
        Self { builds }
    }
}

impl QueryHandler<ListBuildRuns> for ListBuildRunsHandler {
    fn execute(
        &self,
        query: ListBuildRuns,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<BuildRun>>>> {
        let builds = Arc::clone(&self.builds);
        Box::pin(async move {
            Ok(builds
                .list(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                    query.limit,
                )
                .await
                .map_err(Into::into))
        })
    }
}
