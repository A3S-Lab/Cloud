use crate::modules::identity::domain::repositories::TenantSupportGrantRecord;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{ApiTokenId, PrincipalId, TenantSupportGrantId};
use a3s_boot::Query;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GetTenantSupportGrant {
    pub grant_id: TenantSupportGrantId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl Query for GetTenantSupportGrant {
    type Output = ApplicationResult<TenantSupportGrantRecord>;
}
