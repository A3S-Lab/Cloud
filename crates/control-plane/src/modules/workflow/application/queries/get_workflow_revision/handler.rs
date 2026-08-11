use super::GetWorkflowRevision;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::domain::{IWorkflowDefinitionRepository, WorkflowRevision};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetWorkflowRevisionHandler {
    repository: Arc<dyn IWorkflowDefinitionRepository>,
}

impl GetWorkflowRevisionHandler {
    pub fn new(repository: Arc<dyn IWorkflowDefinitionRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetWorkflowRevision> for GetWorkflowRevisionHandler {
    fn execute(
        &self,
        query: GetWorkflowRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<WorkflowRevision>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository
                .find_revision(
                    query.organization_id,
                    query.workflow_definition_id,
                    query.workflow_revision_id,
                )
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "Workflow revision not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
