use crate::modules::forms::application::resource_access::FormAccess;
use crate::modules::forms::domain::FormDraft;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListFormDrafts {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub access: FormAccess,
}

impl Query for ListFormDrafts {
    type Output = ApplicationResult<Vec<FormDraft>>;
}
