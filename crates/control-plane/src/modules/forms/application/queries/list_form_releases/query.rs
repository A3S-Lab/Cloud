use crate::modules::forms::domain::FormRelease;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{FormId, OrganizationId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListFormReleases {
    pub organization_id: OrganizationId,
    pub form_id: FormId,
}

impl Query for ListFormReleases {
    type Output = ApplicationResult<Vec<FormRelease>>;
}
