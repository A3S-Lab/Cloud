use crate::modules::executions::domain::{Execution, IExecutionRepository};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{ExecutionId, OrganizationId, RepositoryError};
use std::sync::Arc;

/// Resolves an indirect Execution identifier through the owning repository before grant
/// evaluation.
///
/// Identity owns grant semantics; Executions owns the canonical project/environment identity.
/// Missing and denied identifiers intentionally share the same not-found contract.
#[derive(Clone)]
pub(crate) struct ExecutionResourceAccess {
    executions: Arc<dyn IExecutionRepository>,
}

impl ExecutionResourceAccess {
    pub fn new(executions: Arc<dyn IExecutionRepository>) -> Self {
        Self { executions }
    }

    pub async fn execution(
        &self,
        organization_id: OrganizationId,
        execution_id: ExecutionId,
        evaluator: &ResourceAccessEvaluator,
    ) -> ApplicationResult<Execution> {
        let execution = match self.executions.find(organization_id, execution_id).await {
            Ok(Some(execution)) => execution,
            Ok(None) | Err(RepositoryError::NotFound) => return Err(not_found()),
            Err(error) => return Err(error.into()),
        };
        if !evaluator.allows(ResourceGrantScope::Environment {
            project_id: execution.project_id,
            environment_id: execution.environment_id,
        }) || execution.is_bound_task()
        {
            return Err(not_found());
        }
        Ok(execution)
    }
}

fn not_found() -> ApplicationError {
    ApplicationError::NotFound("execution not found".into())
}
