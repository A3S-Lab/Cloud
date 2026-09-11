use super::ListExecutions;
use crate::modules::executions::domain::{Execution, IExecutionRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListExecutionsHandler {
    executions: Arc<dyn IExecutionRepository>,
}

impl ListExecutionsHandler {
    pub fn new(executions: Arc<dyn IExecutionRepository>) -> Self {
        Self { executions }
    }
}

impl QueryHandler<ListExecutions> for ListExecutionsHandler {
    fn execute(
        &self,
        query: ListExecutions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<Execution>>>> {
        let executions = Arc::clone(&self.executions);
        Box::pin(async move {
            if query.limit == 0 || query.limit > 1_000 {
                return Ok(Err(ApplicationError::Invalid(
                    "execution list limit must be between 1 and 1000".into(),
                )));
            }
            if !query
                .access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "executions not found".into(),
                )));
            }
            Ok(executions
                .list(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                    query.limit,
                )
                .await
                .map_err(Into::into))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::executions::application::resource_access::{
        ExecutionAccess, ExecutionAccessScope,
    };
    use crate::modules::executions::infrastructure::InMemoryExecutionRepository;
    use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
    use a3s_boot::ModuleRef;

    #[tokio::test]
    async fn restricted_query_fails_closed_before_listing_an_ungranted_environment() {
        let project_id = ProjectId::new();
        let handler = ListExecutionsHandler::new(Arc::new(InMemoryExecutionRepository::new()));
        let result = handler
            .execute(
                ListExecutions {
                    organization_id: OrganizationId::new(),
                    project_id,
                    environment_id: EnvironmentId::new(),
                    limit: 50,
                    access: ExecutionAccess::restricted([ExecutionAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert!(matches!(
            result,
            Err(ApplicationError::NotFound(message)) if message == "executions not found"
        ));
    }
}
