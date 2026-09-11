use super::ListWorkflowGoals;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
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
            if !query.access.project_is_visible(query.project_id) {
                return Ok(Err(ApplicationError::NotFound("project not found".into())));
            }
            Ok(repository
                .list(query.organization_id, query.project_id)
                .await
                .map_err(Into::into))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
    use crate::modules::workflow::InMemoryWorkflowGoalRepository;
    use crate::modules::workflow::application::resource_access::{
        WorkflowAccess, WorkflowAccessScope,
    };
    use a3s_boot::ModuleRef;

    #[tokio::test]
    async fn restricted_query_fails_closed_before_listing_an_ungranted_project() {
        let handler =
            ListWorkflowGoalsHandler::new(Arc::new(InMemoryWorkflowGoalRepository::new()));
        let result = handler
            .execute(
                ListWorkflowGoals {
                    organization_id: OrganizationId::new(),
                    project_id: ProjectId::new(),
                    access: WorkflowAccess::restricted([WorkflowAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert!(matches!(
            result,
            Err(ApplicationError::NotFound(message)) if message == "project not found"
        ));
    }
}
