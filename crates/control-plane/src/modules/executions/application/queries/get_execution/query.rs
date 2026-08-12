use crate::modules::executions::domain::Execution;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{ExecutionId, OrganizationId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetExecution {
    pub organization_id: OrganizationId,
    pub execution_id: ExecutionId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetExecution {
    type Output = ApplicationResult<Execution>;
}
