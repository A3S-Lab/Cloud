use super::GetBuildRunLogs;
use crate::modules::artifacts::application::resource_access::BuildRunResourceAccess;
use crate::modules::artifacts::application::{
    BuildLogPage, BuildLogQueryError, BuildLogReadRequest, BuildRunLogPage, IBuildLogQueryPort,
    MAX_BUILD_LOG_PAGE_SIZE,
};
use crate::modules::artifacts::domain::IBuildRunRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use async_trait::async_trait;
use std::sync::Arc;

const BOX_BUILD_LOGS_UNAVAILABLE: &str =
    "durable Box build logs are unavailable until Box exposes its build log contract";

pub struct GetBuildRunLogsHandler {
    builds: Arc<dyn IBuildRunRepository>,
    logs: Arc<dyn IBuildLogQueryPort>,
}

impl GetBuildRunLogsHandler {
    pub fn new(builds: Arc<dyn IBuildRunRepository>) -> Self {
        Self::with_log_query_port(builds, Arc::new(UnavailableBuildLogQueryPort))
    }

    pub fn with_log_query_port(
        builds: Arc<dyn IBuildRunRepository>,
        logs: Arc<dyn IBuildLogQueryPort>,
    ) -> Self {
        Self { builds, logs }
    }
}

struct UnavailableBuildLogQueryPort;

#[async_trait]
impl IBuildLogQueryPort for UnavailableBuildLogQueryPort {
    async fn read(
        &self,
        _request: BuildLogReadRequest,
    ) -> Result<BuildLogPage, BuildLogQueryError> {
        Err(BuildLogQueryError::Unavailable(
            BOX_BUILD_LOGS_UNAVAILABLE.into(),
        ))
    }
}

impl QueryHandler<GetBuildRunLogs> for GetBuildRunLogsHandler {
    fn execute(
        &self,
        query: GetBuildRunLogs,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<BuildRunLogPage>>> {
        let builds = Arc::clone(&self.builds);
        let logs = Arc::clone(&self.logs);
        Box::pin(async move {
            if query.limit == 0 || query.limit > MAX_BUILD_LOG_PAGE_SIZE {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "build log limit must be between 1 and {MAX_BUILD_LOG_PAGE_SIZE}"
                ))));
            }
            let build_run = match BuildRunResourceAccess::new(builds)
                .build_run(
                    query.organization_id,
                    query.build_run_id,
                    &query.resource_access,
                    "build run logs not found",
                )
                .await
            {
                Ok(build_run) => build_run,
                Err(error) => return Ok(Err(error)),
            };
            let request = BuildLogReadRequest {
                organization_id: build_run.organization_id,
                build_run_id: build_run.id,
                operation_id: build_run.operation_id,
                attempt: build_run.attempt,
                after_sequence: query.after_sequence,
                limit: query.limit,
                stream: query.stream,
            };
            let page = match logs.read(request.clone()).await {
                Ok(page) => page,
                Err(error) => return Ok(Err(map_log_query_error(error))),
            };
            if let Err(error) = page.validate_for(&request) {
                return Ok(Err(ApplicationError::Internal(format!(
                    "build log provider returned an invalid page: {error}"
                ))));
            }
            let (records, next_after_sequence) = page.into_parts();
            Ok(Ok(BuildRunLogPage {
                build_run_id: build_run.id,
                operation_id: build_run.operation_id,
                generation: u64::from(build_run.attempt),
                records,
                next_after_sequence,
            }))
        })
    }
}

fn map_log_query_error(error: BuildLogQueryError) -> ApplicationError {
    match error {
        BuildLogQueryError::Unavailable(message) => ApplicationError::Unavailable(message),
        BuildLogQueryError::Internal(message) => ApplicationError::Internal(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::application::{BuildLogData, BuildLogRecord, BuildLogStream};
    use crate::modules::artifacts::infrastructure::InMemoryBuildRunRepository;
    use crate::modules::identity::domain::services::ResourceAccessEvaluator;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, OrganizationId, ProjectId, SourceRevisionId,
    };
    use a3s_boot::ModuleRef;
    use chrono::Utc;
    use std::sync::Mutex;

    #[derive(Clone)]
    struct StaticBuildLogQueryPort {
        requests: Arc<Mutex<Vec<BuildLogReadRequest>>>,
        result: Result<BuildLogPage, BuildLogQueryError>,
    }

    #[async_trait]
    impl IBuildLogQueryPort for StaticBuildLogQueryPort {
        async fn read(
            &self,
            request: BuildLogReadRequest,
        ) -> Result<BuildLogPage, BuildLogQueryError> {
            self.requests.lock().expect("request lock").push(request);
            self.result.clone()
        }
    }

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
            .reserve_pending(1)
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
                    resource_access: ResourceAccessEvaluator::organization_wide(),
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

    #[tokio::test]
    async fn injected_port_receives_only_artifacts_identity_and_returns_local_records() {
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
            .reserve_pending(1)
            .await
            .expect("reserve build")
            .pop()
            .expect("one build");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let page = BuildLogPage::new(
            vec![BuildLogRecord::Data(
                BuildLogData::new(
                    "box-cursor-7".into(),
                    7,
                    1_000,
                    BuildLogStream::Stdout,
                    "compiled".into(),
                )
                .expect("valid build log data"),
            )],
            Some(7),
        )
        .expect("valid build log page");
        let handler = GetBuildRunLogsHandler::with_log_query_port(
            builds,
            Arc::new(StaticBuildLogQueryPort {
                requests: Arc::clone(&requests),
                result: Ok(page),
            }),
        );

        let result = handler
            .execute(
                GetBuildRunLogs {
                    organization_id,
                    build_run_id: build.id,
                    resource_access: ResourceAccessEvaluator::organization_wide(),
                    after_sequence: Some(6),
                    limit: 100,
                    stream: Some(BuildLogStream::Stdout),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("execute build log query")
            .expect("build log page");

        assert_eq!(result.build_run_id, build.id);
        assert_eq!(result.operation_id, build.operation_id);
        assert_eq!(result.generation, u64::from(build.attempt));
        assert_eq!(result.records.len(), 1);
        assert_eq!(result.next_after_sequence, Some(7));
        assert_eq!(
            requests.lock().expect("request lock").as_slice(),
            &[BuildLogReadRequest {
                organization_id,
                build_run_id: build.id,
                operation_id: build.operation_id,
                attempt: build.attempt,
                after_sequence: Some(6),
                limit: 100,
                stream: Some(BuildLogStream::Stdout),
            }]
        );
    }

    #[tokio::test]
    async fn handler_rejects_a_port_page_that_breaks_the_requested_filter() {
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
            .reserve_pending(1)
            .await
            .expect("reserve build")
            .pop()
            .expect("one build");
        let page = BuildLogPage::new(
            vec![BuildLogRecord::Data(
                BuildLogData::new(
                    "box-cursor-1".into(),
                    1,
                    1_000,
                    BuildLogStream::Stderr,
                    "failure".into(),
                )
                .expect("valid build log data"),
            )],
            None,
        )
        .expect("structurally valid page");
        let handler = GetBuildRunLogsHandler::with_log_query_port(
            builds,
            Arc::new(StaticBuildLogQueryPort {
                requests: Arc::new(Mutex::new(Vec::new())),
                result: Ok(page),
            }),
        );

        let result = handler
            .execute(
                GetBuildRunLogs {
                    organization_id,
                    build_run_id: build.id,
                    resource_access: ResourceAccessEvaluator::organization_wide(),
                    after_sequence: None,
                    limit: 100,
                    stream: Some(BuildLogStream::Stdout),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("execute build log query");

        assert_eq!(
            result,
            Err(ApplicationError::Internal(
                "build log provider returned an invalid page: build log page violated the requested stream filter".into()
            ))
        );
    }
}
