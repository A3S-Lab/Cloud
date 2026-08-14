use super::GetWorkflowRunVariables;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::application::resource_access;
use crate::modules::workflow::domain::{
    IWorkflowRunRepository, IWorkflowRunVariableReader, WorkflowRunVariableInspection,
};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetWorkflowRunVariablesHandler {
    repository: Arc<dyn IWorkflowRunRepository>,
    variables: Arc<dyn IWorkflowRunVariableReader>,
}

impl GetWorkflowRunVariablesHandler {
    pub fn new(
        repository: Arc<dyn IWorkflowRunRepository>,
        variables: Arc<dyn IWorkflowRunVariableReader>,
    ) -> Self {
        Self {
            repository,
            variables,
        }
    }
}

impl QueryHandler<GetWorkflowRunVariables> for GetWorkflowRunVariablesHandler {
    fn execute(
        &self,
        query: GetWorkflowRunVariables,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<WorkflowRunVariableInspection>>,
    > {
        let repository = Arc::clone(&self.repository);
        let variables = Arc::clone(&self.variables);
        Box::pin(async move {
            let record = match resource_access::workflow_run(
                repository.as_ref(),
                query.organization_id,
                query.workflow_run_id,
                &query.resource_access,
            )
            .await
            {
                Ok(record) => record,
                Err(error) => return Ok(Err(error)),
            };
            if record.run.execution_input.variable_contract.is_none() {
                return Ok(Err(ApplicationError::Conflict(
                    "WorkflowRun does not carry an exact typed variable contract".into(),
                )));
            }
            Ok(variables
                .inspect(&record)
                .await
                .map_err(ApplicationError::Unavailable))
        })
    }
}
