use super::{ListWorkflowRuns, WORKFLOW_RUN_LIST_MAX_LIMIT};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::domain::{IWorkflowRunRepository, WorkflowRunRecord};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListWorkflowRunsHandler {
    repository: Arc<dyn IWorkflowRunRepository>,
}

impl ListWorkflowRunsHandler {
    pub fn new(repository: Arc<dyn IWorkflowRunRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListWorkflowRuns> for ListWorkflowRunsHandler {
    fn execute(
        &self,
        query: ListWorkflowRuns,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<WorkflowRunRecord>>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            if !query.access.project_is_visible(query.project_id) {
                return Ok(Err(ApplicationError::NotFound("project not found".into())));
            }
            if query.limit == 0 || query.limit > WORKFLOW_RUN_LIST_MAX_LIMIT {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "WorkflowRun limit must be between 1 and {WORKFLOW_RUN_LIST_MAX_LIMIT}"
                ))));
            }
            Ok(repository
                .list(query.organization_id, query.project_id, query.limit)
                .await
                .map_err(Into::into))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
    use crate::modules::workflow::InMemoryWorkflowRunRepository;
    use crate::modules::workflow::application::resource_access::{
        WorkflowAccess, WorkflowAccessScope,
    };
    use a3s_boot::ModuleRef;

    #[tokio::test]
    async fn restricted_query_fails_closed_before_listing_an_ungranted_project() {
        let handler = ListWorkflowRunsHandler::new(Arc::new(InMemoryWorkflowRunRepository::new()));
        let result = handler
            .execute(
                ListWorkflowRuns {
                    organization_id: OrganizationId::new(),
                    project_id: ProjectId::new(),
                    access: WorkflowAccess::restricted([WorkflowAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                    limit: 10,
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
