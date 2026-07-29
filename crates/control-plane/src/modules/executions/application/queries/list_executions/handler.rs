use super::ListExecutions;
use crate::modules::executions::domain::{Execution, IExecutionRepository};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListExecutionsHandler {
    executions: Arc<dyn IExecutionRepository>,
}

impl ListExecutionsHandler {
    pub fn new(executions: Arc<dyn IExecutionRepository>) -> Self {
        Self { executions }
    }
}

impl QueryHandler<ListExecutions> for ListExecutionsHandler {
    fn execute(
        &self,
        query: ListExecutions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<Execution>>>> {
        let executions = Arc::clone(&self.executions);
        Box::pin(async move {
            if query.limit == 0 || query.limit > 1_000 {
                return Ok(Err(
                    crate::modules::shared_kernel::application::ApplicationError::Invalid(
                        "execution list limit must be between 1 and 1000".into(),
                    ),
                ));
            }
            Ok(executions
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
