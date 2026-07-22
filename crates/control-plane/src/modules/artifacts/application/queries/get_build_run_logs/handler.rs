use super::GetBuildRunLogs;
use crate::modules::artifacts::application::BuildRunLogPage;
use crate::modules::artifacts::domain::{BuildRun, IBuildRunRepository};
use crate::modules::fleet::application::{NodeLogReadQuery, NodeLogReader};
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::fleet::domain::services::ILogChunkStore;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetBuildRunLogsHandler {
    builds: Arc<dyn IBuildRunRepository>,
    logs: NodeLogReader,
}

impl GetBuildRunLogsHandler {
    pub fn new(
        builds: Arc<dyn IBuildRunRepository>,
        metadata: Arc<dyn INodeControlRepository>,
        objects: Arc<dyn ILogChunkStore>,
    ) -> Self {
        Self {
            builds,
            logs: NodeLogReader::new(metadata, objects),
        }
    }
}

impl QueryHandler<GetBuildRunLogs> for GetBuildRunLogsHandler {
    fn execute(
        &self,
        query: GetBuildRunLogs,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<BuildRunLogPage>>> {
        let builds = Arc::clone(&self.builds);
        let logs = self.logs.clone();
        Box::pin(async move {
            if query.limit == 0 || query.limit > 256 {
                return Ok(Err(ApplicationError::Invalid(
                    "build log limit must be between 1 and 256".into(),
                )));
            }
            let build = match builds.find(query.organization_id, query.build_run_id).await {
                Ok(build) => build,
                Err(RepositoryError::NotFound) => return Ok(Err(logs_not_found())),
                Err(error) => return Ok(Err(error.into())),
            };
            let Some(node_id) = build.node_id else {
                return Ok(Ok(BuildRunLogPage {
                    build_run_id: build.id,
                    operation_id: build.operation_id,
                    generation: BuildRun::RUNTIME_GENERATION,
                    records: Vec::new(),
                    next_after_sequence: None,
                }));
            };
            let page = match logs
                .read(NodeLogReadQuery {
                    node_id,
                    unit_id: build.runtime_unit_id(),
                    generation: BuildRun::RUNTIME_GENERATION,
                    after_sequence: query.after_sequence,
                    limit: query.limit,
                    stream: query.stream,
                })
                .await
            {
                Ok(page) => page,
                Err(error) => return Ok(Err(error)),
            };
            Ok(Ok(BuildRunLogPage {
                build_run_id: build.id,
                operation_id: build.operation_id,
                generation: page.generation,
                records: page.records,
                next_after_sequence: page.next_after_sequence,
            }))
        })
    }
}

fn logs_not_found() -> ApplicationError {
    ApplicationError::NotFound("build run logs not found".into())
}
