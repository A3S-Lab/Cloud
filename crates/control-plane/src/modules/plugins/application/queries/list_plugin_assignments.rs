use crate::modules::plugins::domain::entities::PluginAssignment;
use crate::modules::plugins::domain::repositories::IPluginAssignmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListPluginAssignments {
    pub organization_id: OrganizationId,
    pub environment_id: EnvironmentId,
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
            Ok(assignments
                .list_for_environment(query.organization_id, query.environment_id)
                .await
                .map_err(ApplicationError::from))
        })
    }
}
