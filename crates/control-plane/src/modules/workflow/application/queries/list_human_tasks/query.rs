use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use crate::modules::workflow::domain::{HumanTaskRecord, HumanTaskStatus};
use a3s_boot::Query;

pub const HUMAN_TASK_LIST_MAX_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct ListHumanTasks {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub status: Option<HumanTaskStatus>,
    pub limit: usize,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ListHumanTasks {
    type Output = ApplicationResult<Vec<HumanTaskRecord>>;
}
