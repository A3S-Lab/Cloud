use super::GetWorkflowGoal;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::domain::{IWorkflowGoalRepository, WorkflowGoalRecord};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetWorkflowGoalHandler {
    repository: Arc<dyn IWorkflowGoalRepository>,
}

impl GetWorkflowGoalHandler {
    pub fn new(repository: Arc<dyn IWorkflowGoalRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetWorkflowGoal> for GetWorkflowGoalHandler {
    fn execute(
        &self,
        query: GetWorkflowGoal,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<WorkflowGoalRecord>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository
                .find(query.organization_id, query.workflow_goal_id)
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "WorkflowGoal not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
