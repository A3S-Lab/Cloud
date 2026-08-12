use super::{CreateExecutionCommand, CreateExecutionResult};
use crate::modules::executions::application::execution_creator::{
    ExecutionCreation, ExecutionCreator,
};
use crate::modules::executions::domain::IExecutionRepository;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CommandHandler, CqrsContext};
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
        let creator =
            ExecutionCreator::new(Arc::clone(&self.environments), Arc::clone(&self.executions));
        Box::pin(async move {
            Ok(creator
                .create(ExecutionCreation {
                    organization_id: command.organization_id,
                    project_id: command.project_id,
                    environment_id: command.environment_id,
                    template: command.template,
                    workflow: None,
                    idempotency_key: command.idempotency_key,
                    request_id: command.request_id,
                    requested_at: command.requested_at,
                })
                .await)
        })
    }
}
