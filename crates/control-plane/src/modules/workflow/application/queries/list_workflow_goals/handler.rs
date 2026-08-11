use super::ListWorkflowGoals;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::workflow::domain::{IWorkflowGoalRepository, WorkflowGoalRecord};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListWorkflowGoalsHandler {
    repository: Arc<dyn IWorkflowGoalRepository>,
}

impl ListWorkflowGoalsHandler {
    pub fn new(repository: Arc<dyn IWorkflowGoalRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListWorkflowGoals> for ListWorkflowGoalsHandler {
    fn execute(
        &self,
        query: ListWorkflowGoals,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<WorkflowGoalRecord>>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            Ok(repository
                .list(query.organization_id, query.project_id)
                .await
                .map_err(Into::into))
        })
    }
}
