use super::GetExecution;
use crate::modules::executions::domain::{Execution, IExecutionRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetExecutionHandler {
    executions: Arc<dyn IExecutionRepository>,
}

impl GetExecutionHandler {
    pub fn new(executions: Arc<dyn IExecutionRepository>) -> Self {
        Self { executions }
    }
}

impl QueryHandler<GetExecution> for GetExecutionHandler {
    fn execute(
        &self,
        query: GetExecution,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Execution>>> {
        let executions = Arc::clone(&self.executions);
        Box::pin(async move {
            Ok(
                match executions
                    .find(query.organization_id, query.execution_id)
                    .await
                {
                    Ok(Some(execution)) => Ok(execution),
                    Ok(None) => Err(ApplicationError::NotFound("execution not found".into())),
                    Err(error) => Err(error.into()),
                },
            )
        })
    }
}
