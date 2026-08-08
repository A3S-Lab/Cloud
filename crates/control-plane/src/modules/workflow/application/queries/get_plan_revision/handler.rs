use super::GetPlanRevision;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::domain::{IWorkflowGoalRepository, PlanRevision};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetPlanRevisionHandler {
    repository: Arc<dyn IWorkflowGoalRepository>,
}

impl GetPlanRevisionHandler {
    pub fn new(repository: Arc<dyn IWorkflowGoalRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetPlanRevision> for GetPlanRevisionHandler {
    fn execute(
        &self,
        query: GetPlanRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PlanRevision>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository
                .find_plan_revision(
                    query.organization_id,
                    query.workflow_goal_id,
                    query.plan_revision_id,
                )
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "PlanRevision not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
