use super::GetWorkflowGoal;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::workflow::application::resource_access;
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
            Ok(resource_access::workflow_goal(
                repository.as_ref(),
                query.organization_id,
                query.workflow_goal_id,
                &query.resource_access,
            )
            .await)
        })
    }
}
