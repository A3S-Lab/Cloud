use super::{CreateExecutionCommand, CreateExecutionResult};
use crate::modules::executions::domain::events::ExecutionRequested;
use crate::modules::executions::domain::{CreateExecution, Execution, IExecutionRepository};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{ExecutionId, IdempotencyRequest};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CreateExecutionHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    executions: Arc<dyn IExecutionRepository>,
}

impl CreateExecutionHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        executions: Arc<dyn IExecutionRepository>,
    ) -> Self {
        Self {
            environments,
            executions,
        }
    }
}

impl CommandHandler<CreateExecutionCommand> for CreateExecutionHandler {
    fn execute(
        &self,
        command: CreateExecutionCommand,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<CreateExecutionResult>>>
    {
        let environments = Arc::clone(&self.environments);
        let executions = Arc::clone(&self.executions);
        Box::pin(async move {
            match environments
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            if let Err(error) = command.template.validate() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "projectId": command.project_id,
                "environmentId": command.environment_id,
                "template": command.template,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/executions",
                    command.organization_id, command.project_id, command.environment_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if let Some(replay) = match executions.replay(&idempotency).await {
                Ok(replay) => replay,
                Err(error) => return Ok(Err(error.into())),
            } {
                return Ok(Ok(CreateExecutionResult {
                    execution: replay,
                    replayed: true,
                }));
            }
            let execution = match Execution::create(
                command.organization_id,
                command.project_id,
                command.environment_id,
                ExecutionId::new(),
                command.template,
                command.requested_at,
            ) {
                Ok(execution) => execution,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = ExecutionRequested::envelope(&execution, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            match executions
                .create(CreateExecution {
                    execution,
                    idempotency,
                    event,
                })
                .await
            {
                Ok(write) => Ok(Ok(CreateExecutionResult {
                    execution: write.execution,
                    replayed: write.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
