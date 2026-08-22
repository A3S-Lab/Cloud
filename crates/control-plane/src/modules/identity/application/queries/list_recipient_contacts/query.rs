use crate::modules::identity::domain::entities::RecipientContactRecord;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListRecipientContacts {
    pub organization_id: OrganizationId,
    pub actor_principal_id: PrincipalId,
}

impl Query for ListRecipientContacts {
    type Output = ApplicationResult<Vec<RecipientContactRecord>>;
}
