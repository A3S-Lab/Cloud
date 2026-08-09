use crate::modules::forms::domain::FormRelease;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{FormId, FormReleaseId, OrganizationId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetFormRelease {
    pub organization_id: OrganizationId,
    pub form_id: FormId,
    pub release_id: FormReleaseId,
}

impl Query for GetFormRelease {
    type Output = ApplicationResult<FormRelease>;
}
