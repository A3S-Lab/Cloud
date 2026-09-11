use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{HumanTaskId, OrganizationId, PrincipalId};
use crate::modules::workflow::domain::HumanTaskRecord;
use a3s_boot::Query;
use crate::modules::workflow::application::WorkflowAccess;

#[derive(Debug, Clone)]
pub struct GetHumanTask {
    pub organization_id: OrganizationId,
    pub human_task_id: HumanTaskId,
    pub actor_principal_id: PrincipalId,
    pub access: WorkflowAccess,
}

impl Query for GetHumanTask {
    type Output = ApplicationResult<HumanTaskRecord>;
}
