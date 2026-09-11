use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use async_trait::async_trait;

/// Consumer-owned port for the minimum Organization evidence required before an
/// enrollment token can be issued. No Identity aggregate crosses this boundary.
#[async_trait]
pub trait IFleetOrganizationAccess: Send + Sync {
    async fn require_organization(&self, organization_id: OrganizationId) -> ApplicationResult<()>;
}
