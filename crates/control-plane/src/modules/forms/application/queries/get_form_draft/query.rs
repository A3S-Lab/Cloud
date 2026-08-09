use crate::modules::forms::domain::FormDraft;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{FormId, OrganizationId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetFormDraft {
    pub organization_id: OrganizationId,
    pub form_id: FormId,
}

impl Query for GetFormDraft {
    type Output = ApplicationResult<FormDraft>;
}
