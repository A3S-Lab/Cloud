use crate::modules::identity::domain::entities::ApiToken;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{ApiTokenId, OrganizationId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetApiToken {
    pub organization_id: OrganizationId,
    pub token_id: ApiTokenId,
}

impl Query for GetApiToken {
    type Output = ApplicationResult<ApiToken>;
}
