use crate::modules::plugins::application::PluginAccess;
use crate::modules::plugins::domain::entities::PluginAssignment;
use crate::modules::plugins::domain::repositories::IPluginAssignmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListPluginAssignments {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub access: PluginAccess,
}

impl Query for ListPluginAssignments {
    type Output = ApplicationResult<Vec<PluginAssignment>>;
}

pub struct ListPluginAssignmentsHandler {
    assignments: Arc<dyn IPluginAssignmentRepository>,
}

impl ListPluginAssignmentsHandler {
    pub fn new(assignments: Arc<dyn IPluginAssignmentRepository>) -> Self {
        Self { assignments }
    }
}

impl QueryHandler<ListPluginAssignments> for ListPluginAssignmentsHandler {
    fn execute(
        &self,
        query: ListPluginAssignments,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<PluginAssignment>>>>
    {
        let assignments = Arc::clone(&self.assignments);
        Box::pin(async move {
            if !query
                .access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "plugin assignments not found".into(),
                )));
            }
            match assignments
                .list_for_environment(query.organization_id, query.environment_id)
                .await
            {
                Ok(values) => Ok(Ok(values
                    .into_iter()
                    .filter(|assignment| assignment.project_id == query.project_id)
                    .collect())),
                Err(error) => Ok(Err(ApplicationError::from(error))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::plugins::InMemoryPluginAssignmentRepository;
    use crate::modules::plugins::application::resource_access::{PluginAccess, PluginAccessScope};
    use a3s_boot::ModuleRef;

    #[tokio::test]
    async fn restricted_query_fails_closed_before_listing_an_ungranted_environment() {
        let handler =
            ListPluginAssignmentsHandler::new(Arc::new(InMemoryPluginAssignmentRepository::new()));
        let result = handler
            .execute(
                ListPluginAssignments {
                    organization_id: OrganizationId::new(),
                    project_id: ProjectId::new(),
                    environment_id: EnvironmentId::new(),
                    access: PluginAccess::restricted([PluginAccessScope::Environment {
                        project_id: ProjectId::new(),
                        environment_id: EnvironmentId::new(),
                    }]),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert!(matches!(
            result,
            Err(ApplicationError::NotFound(message))
                if message == "plugin assignments not found"
        ));
    }
}
