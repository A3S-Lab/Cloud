use super::GetBuildRunLogs;
use crate::modules::artifacts::application::BuildRunLogPage;
use crate::modules::artifacts::domain::IBuildRunRepository;
use crate::modules::fleet::application::MAX_LOG_PAGE_SIZE;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

const BOX_BUILD_LOGS_UNAVAILABLE: &str =
    "durable Box build logs are unavailable until Box exposes its build log contract";

pub struct GetBuildRunLogsHandler {
    builds: Arc<dyn IBuildRunRepository>,
}

impl GetBuildRunLogsHandler {
    pub fn new(builds: Arc<dyn IBuildRunRepository>) -> Self {
        Self { builds }
    }
}

impl QueryHandler<GetBuildRunLogs> for GetBuildRunLogsHandler {
    fn execute(
        &self,
        query: GetBuildRunLogs,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<BuildRunLogPage>>> {
        let builds = Arc::clone(&self.builds);
        Box::pin(async move {
            if query.limit == 0 || query.limit > MAX_LOG_PAGE_SIZE {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "build log limit must be between 1 and {MAX_LOG_PAGE_SIZE}"
                ))));
            }
            match builds.find(query.organization_id, query.build_run_id).await {
                Ok(_) => {}
                Err(RepositoryError::NotFound) => return Ok(Err(logs_not_found())),
                Err(error) => return Ok(Err(error.into())),
            }
            Ok(Err(ApplicationError::Unavailable(
                BOX_BUILD_LOGS_UNAVAILABLE.into(),
            )))
        })
    }
}

fn logs_not_found() -> ApplicationError {
    ApplicationError::NotFound("build run logs not found".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::infrastructure::InMemoryBuildRunRepository;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, OrganizationId, ProjectId, SourceRevisionId,
    };
    use a3s_boot::ModuleRef;
    use chrono::Utc;

    #[tokio::test]
    async fn existing_build_reports_box_logs_unavailable_instead_of_fake_empty_success() {
        let builds = Arc::new(InMemoryBuildRunRepository::new());
        let organization_id = OrganizationId::new();
        let accepted_at = Utc::now();
        builds
            .add_source_revision(
                organization_id,
                ProjectId::new(),
                EnvironmentId::new(),
                SourceRevisionId::new(),
                accepted_at,
            )
            .await;
        let build = builds
            .reserve_pending(1, accepted_at)
            .await
            .expect("reserve build")
            .pop()
            .expect("one build");
        let handler = GetBuildRunLogsHandler::new(builds);

        let result = handler
            .execute(
                GetBuildRunLogs {
                    organization_id,
                    build_run_id: build.id,
                    after_sequence: None,
                    limit: 100,
                    stream: None,
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("execute build log query");

        assert_eq!(
            result,
            Err(ApplicationError::Unavailable(
                BOX_BUILD_LOGS_UNAVAILABLE.into()
            ))
        );
    }
}
