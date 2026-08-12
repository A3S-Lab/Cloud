use crate::modules::forms::domain::FormRelease;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{FormId, FormReleaseId, OrganizationId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetFormRelease {
    pub organization_id: OrganizationId,
    pub form_id: FormId,
    pub release_id: FormReleaseId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetFormRelease {
    type Output = ApplicationResult<FormRelease>;
}
