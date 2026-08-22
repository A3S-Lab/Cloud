use crate::modules::identity::domain::entities::RecipientContactRecord;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, RecipientContactId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetRecipientContact {
    pub organization_id: OrganizationId,
    pub actor_principal_id: PrincipalId,
    pub contact_id: RecipientContactId,
}

impl Query for GetRecipientContact {
    type Output = ApplicationResult<RecipientContactRecord>;
}
