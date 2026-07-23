use super::{RetryBuildRun, RetryBuildRunResult};
use crate::modules::artifacts::domain::{BuildRun, IBuildRunRepository, RequestBuildRetryBundle};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, RepositoryError};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct RetryBuildRunHandler {
    builds: Arc<dyn IBuildRunRepository>,
}

impl RetryBuildRunHandler {
    pub fn new(builds: Arc<dyn IBuildRunRepository>) -> Self {
        Self { builds }
    }
}

impl CommandHandler<RetryBuildRun> for RetryBuildRunHandler {
    fn execute(
        &self,
        command: RetryBuildRun,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<RetryBuildRunResult>>>
    {
        let builds = Arc::clone(&self.builds);
        Box::pin(async move {
            let previous = match builds
                .find(command.organization_id, command.build_run_id)
                .await
            {
                Ok(build_run) => build_run,
                Err(RepositoryError::NotFound) => return Ok(Err(build_not_found())),
                Err(error) => return Ok(Err(error.into())),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "buildRunId": command.build_run_id,
                "organizationId": command.organization_id,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/build-runs/{}/retry",
                    command.organization_id, command.build_run_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let retry = match BuildRun::retry(&previous, command.requested_at) {
                Ok(retry) => retry,
                Err(error) => {
                    return match builds.replay_retry(&idempotency).await {
                        Ok(Some(replay))
                            if replay.organization_id == command.organization_id
                                && replay.retry_of_build_run_id == Some(command.build_run_id) =>
                        {
                            Ok(Ok(RetryBuildRunResult {
                                build_run: replay,
                                retry_of_build_run_id: command.build_run_id,
                                replayed: true,
                            }))
                        }
                        Ok(Some(_)) => Err(BootError::Internal(
                            "build retry replay changed its identity".into(),
                        )),
                        Ok(None) => Ok(Err(ApplicationError::Conflict(error))),
                        Err(repository_error) => Ok(Err(repository_error.into())),
                    };
                }
            };
            match builds
                .request_retry(RequestBuildRetryBundle {
                    retry,
                    expected_previous_version: previous.aggregate_version,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(RetryBuildRunResult {
                    build_run: result.value,
                    retry_of_build_run_id: command.build_run_id,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

fn build_not_found() -> ApplicationError {
    ApplicationError::NotFound("build run not found".into())
}
