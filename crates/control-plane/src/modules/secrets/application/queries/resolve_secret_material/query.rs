use crate::modules::secrets::application::SecretPlaintext;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{NodeId, OrganizationId};
use a3s_boot::Query;
use a3s_cloud_contracts::CloudSecretReference;

#[derive(Debug, Clone)]
pub struct ResolveSecretMaterial {
    pub organization_id: OrganizationId,
    pub authenticated_node_id: NodeId,
    pub reference: CloudSecretReference,
}

impl Query for ResolveSecretMaterial {
    type Output = ApplicationResult<SecretPlaintext>;
}
