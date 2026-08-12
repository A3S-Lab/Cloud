use super::GetWorkflowDefinition;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::workflow::application::resource_access;
use crate::modules::workflow::domain::{IWorkflowDefinitionRepository, WorkflowDefinition};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetWorkflowDefinitionHandler {
    repository: Arc<dyn IWorkflowDefinitionRepository>,
}

impl GetWorkflowDefinitionHandler {
    pub fn new(repository: Arc<dyn IWorkflowDefinitionRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetWorkflowDefinition> for GetWorkflowDefinitionHandler {
    fn execute(
        &self,
        query: GetWorkflowDefinition,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<WorkflowDefinition>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            Ok(resource_access::workflow_definition(
                repository.as_ref(),
                query.organization_id,
                query.workflow_definition_id,
                &query.resource_access,
            )
            .await)
        })
    }
}
