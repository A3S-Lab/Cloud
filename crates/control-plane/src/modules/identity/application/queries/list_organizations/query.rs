use crate::modules::identity::domain::entities::Organization;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{ApiTokenId, PrincipalId};
use a3s_boot::Query;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ListOrganizations {
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl Query for ListOrganizations {
    type Output = ApplicationResult<Vec<Organization>>;
}
