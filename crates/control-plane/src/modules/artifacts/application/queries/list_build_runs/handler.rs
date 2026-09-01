use super::ListBuildRuns;
use crate::modules::artifacts::domain::{BuildRun, IBuildRunRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
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
            if !query
                .access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "build runs not found".into(),
                )));
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::application::{ArtifactAccess, ArtifactAccessScope};
    use crate::modules::artifacts::infrastructure::InMemoryBuildRunRepository;
    use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
    use a3s_boot::ModuleRef;

    #[tokio::test]
    async fn restricted_query_fails_closed_before_listing_an_ungranted_environment() {
        let project_id = ProjectId::new();
        let handler = ListBuildRunsHandler::new(Arc::new(InMemoryBuildRunRepository::new()));
        let result = handler
            .execute(
                ListBuildRuns {
                    organization_id: OrganizationId::new(),
                    project_id,
                    environment_id: EnvironmentId::new(),
                    access: ArtifactAccess::restricted([ArtifactAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                    limit: 50,
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("execute list query");

        assert_eq!(
            result,
            Err(ApplicationError::NotFound("build runs not found".into()))
        );
    }
}
