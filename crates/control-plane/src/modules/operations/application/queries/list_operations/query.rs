use crate::modules::operations::domain::entities::OperationRecord;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListOperations {
    pub organization_id: OrganizationId,
    pub limit: usize,
}

impl Query for ListOperations {
    type Output = ApplicationResult<Vec<OperationRecord>>;
}
