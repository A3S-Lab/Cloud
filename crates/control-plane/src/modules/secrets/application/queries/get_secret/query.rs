use crate::modules::secrets::application::SecretDetails;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, SecretId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetSecret {
    pub organization_id: OrganizationId,
    pub secret_id: SecretId,
}

impl Query for GetSecret {
    type Output = ApplicationResult<SecretDetails>;
}
