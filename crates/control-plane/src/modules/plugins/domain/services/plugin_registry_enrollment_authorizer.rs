use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum PluginRegistryEnrollmentAuthorizationError {
    #[error("plugin registry enrollment requires an active human organization member")]
    Forbidden,
    #[error("plugin registry enrollment authorization is unavailable: {0}")]
    Unavailable(String),
}

#[async_trait]
pub trait IPluginRegistryEnrollmentAuthorizer: Send + Sync {
    async fn authorize_enrollment(
        &self,
        organization_id: OrganizationId,
        actor_id: PrincipalId,
    ) -> Result<(), PluginRegistryEnrollmentAuthorizationError>;
}
