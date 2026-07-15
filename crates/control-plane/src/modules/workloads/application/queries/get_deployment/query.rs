use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{DeploymentId, OrganizationId};
use crate::modules::workloads::application::queries::DeploymentQueryResult;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetDeployment {
    pub organization_id: OrganizationId,
    pub deployment_id: DeploymentId,
}

impl Query for GetDeployment {
    type Output = ApplicationResult<DeploymentQueryResult>;
}
