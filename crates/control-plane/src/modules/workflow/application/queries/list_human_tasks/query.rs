use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use crate::modules::workflow::domain::{HumanTaskRecord, HumanTaskStatus};
use a3s_boot::Query;
use crate::modules::workflow::application::WorkflowAccess;

pub const HUMAN_TASK_LIST_MAX_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct ListHumanTasks {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub status: Option<HumanTaskStatus>,
    pub limit: usize,
    pub access: WorkflowAccess,
}

impl Query for ListHumanTasks {
    type Output = ApplicationResult<Vec<HumanTaskRecord>>;
}
