use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use async_trait::async_trait;

/// Consumer-owned port for the minimum Organization evidence required by
/// Sources. Principal authorization remains outside this existence boundary.
#[async_trait]
pub trait ISourceOrganizationAccess: Send + Sync {
    async fn require_organization(&self, organization_id: OrganizationId) -> ApplicationResult<()>;
}

/// Consumer-owned port for the minimum Environment ownership evidence required
/// by Sources. No Projects aggregate crosses this boundary.
#[async_trait]
pub trait ISourceEnvironmentAccess: Send + Sync {
    async fn require_environment(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> ApplicationResult<()>;
}
