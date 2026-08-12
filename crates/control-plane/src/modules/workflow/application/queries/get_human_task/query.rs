use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{HumanTaskId, OrganizationId, PrincipalId};
use crate::modules::workflow::domain::HumanTaskRecord;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetHumanTask {
    pub organization_id: OrganizationId,
    pub human_task_id: HumanTaskId,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetHumanTask {
    type Output = ApplicationResult<HumanTaskRecord>;
}
