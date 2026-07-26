use crate::modules::identity::domain::entities::ApiToken;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListApiTokens {
    pub organization_id: OrganizationId,
}

impl Query for ListApiTokens {
    type Output = ApplicationResult<Vec<ApiToken>>;
}
