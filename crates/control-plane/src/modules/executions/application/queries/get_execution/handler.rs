use super::GetExecution;
use crate::modules::executions::application::resource_access::ExecutionResourceAccess;
use crate::modules::executions::domain::{Execution, IExecutionRepository};
use crate::modules::shared_kernel::application::ApplicationResult;
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
            Ok(ExecutionResourceAccess::new(executions)
                .execution(
                    query.organization_id,
                    query.execution_id,
                    &query.resource_access,
                )
                .await)
        })
    }
}
