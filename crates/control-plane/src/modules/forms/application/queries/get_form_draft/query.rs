use crate::modules::forms::domain::FormDraft;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{FormId, OrganizationId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetFormDraft {
    pub organization_id: OrganizationId,
    pub form_id: FormId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetFormDraft {
    type Output = ApplicationResult<FormDraft>;
}
