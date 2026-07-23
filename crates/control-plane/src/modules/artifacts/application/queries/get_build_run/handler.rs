use super::GetBuildRun;
use crate::modules::artifacts::domain::{BuildRun, IBuildRunRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
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
            Ok(
                match builds.find(query.organization_id, query.build_run_id).await {
                    Ok(build_run) => Ok(build_run),
                    Err(RepositoryError::NotFound) => {
                        Err(ApplicationError::NotFound("build run not found".into()))
                    }
                    Err(error) => Err(error.into()),
                },
            )
        })
    }
}
