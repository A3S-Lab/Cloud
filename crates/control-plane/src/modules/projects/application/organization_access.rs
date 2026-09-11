use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use async_trait::async_trait;

/// Consumer-owned port for the minimum Organization evidence required before a
/// Project aggregate can be created. No Identity aggregate crosses this
/// boundary and principal authorization remains a Presentation concern.
#[async_trait]
pub trait IProjectOrganizationAccess: Send + Sync {
    async fn require_organization(&self, organization_id: OrganizationId) -> ApplicationResult<()>;
}
