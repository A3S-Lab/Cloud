use crate::modules::plugins::domain::entities::PluginAssignment;
use crate::modules::plugins::domain::repositories::IPluginAssignmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{OrganizationId, PluginAssignmentId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetPluginAssignment {
    pub organization_id: OrganizationId,
    pub assignment_id: PluginAssignmentId,
}

impl Query for GetPluginAssignment {
    type Output = ApplicationResult<PluginAssignment>;
}

pub struct GetPluginAssignmentHandler {
    assignments: Arc<dyn IPluginAssignmentRepository>,
}

impl GetPluginAssignmentHandler {
    pub fn new(assignments: Arc<dyn IPluginAssignmentRepository>) -> Self {
        Self { assignments }
    }
}

impl QueryHandler<GetPluginAssignment> for GetPluginAssignmentHandler {
    fn execute(
        &self,
        query: GetPluginAssignment,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PluginAssignment>>> {
        let assignments = Arc::clone(&self.assignments);
        Box::pin(async move {
            match assignments
                .find(query.organization_id, query.assignment_id)
                .await
            {
                Ok(Some(assignment)) => Ok(Ok(assignment)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "plugin assignment was not found".into(),
                ))),
                Err(error) => Ok(Err(ApplicationError::from(error))),
            }
        })
    }
}
