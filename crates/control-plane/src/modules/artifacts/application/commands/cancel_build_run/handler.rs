use super::{CancelBuildRun, CancelBuildRunResult};
use crate::modules::artifacts::domain::{IBuildRunRepository, RequestBuildCancellationBundle};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, RepositoryError};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CancelBuildRunHandler {
    builds: Arc<dyn IBuildRunRepository>,
}

impl CancelBuildRunHandler {
    pub fn new(builds: Arc<dyn IBuildRunRepository>) -> Self {
        Self { builds }
    }
}

impl CommandHandler<CancelBuildRun> for CancelBuildRunHandler {
    fn execute(
        &self,
        command: CancelBuildRun,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<CancelBuildRunResult>>>
    {
        let builds = Arc::clone(&self.builds);
        Box::pin(async move {
            let mut build_run = match builds
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
                    "organizations/{}/build-runs/{}/cancellation",
                    command.organization_id, command.build_run_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let expected_version = build_run.aggregate_version;
            if let Err(error) = build_run.request_cancellation(command.requested_at) {
                match builds.replay_cancellation(&idempotency).await {
                    Ok(Some(replay))
                        if replay.organization_id == command.organization_id
                            && replay.id == command.build_run_id =>
                    {
                        return Ok(Ok(CancelBuildRunResult {
                            build_run: replay,
                            replayed: true,
                        }));
                    }
                    Ok(Some(_)) => {
                        return Err(BootError::Internal(
                            "build cancellation replay changed its identity".into(),
                        ));
                    }
                    Ok(None) => {}
                    Err(repository_error) => return Ok(Err(repository_error.into())),
                }
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            match builds
                .request_cancellation(RequestBuildCancellationBundle {
                    build_run,
                    expected_version,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(CancelBuildRunResult {
                    build_run: result.value,
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
