use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use async_trait::async_trait;

/// Plugins-owned proof that one exact actor was admitted to enroll a registry
/// for one exact organization. The Identity model never crosses this boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginRegistryEnrollmentAuthorization {
    organization_id: OrganizationId,
    actor_id: PrincipalId,
}

impl PluginRegistryEnrollmentAuthorization {
    pub(in crate::modules::plugins) fn new(
        organization_id: OrganizationId,
        actor_id: PrincipalId,
    ) -> Result<Self, String> {
        if organization_id.as_uuid().is_nil() || actor_id.as_uuid().is_nil() {
            return Err(
                "plugin registry enrollment authorization requires non-nil identities".into(),
            );
        }
        Ok(Self {
            organization_id,
            actor_id,
        })
    }

    pub fn validate_for(
        &self,
        organization_id: OrganizationId,
        actor_id: PrincipalId,
    ) -> Result<(), String> {
        if self.organization_id != organization_id || self.actor_id != actor_id {
            return Err("plugin registry enrollment authorization scope is inconsistent".into());
        }
        Ok(())
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn actor_id(self) -> PrincipalId {
        self.actor_id
    }
}

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
    ) -> Result<PluginRegistryEnrollmentAuthorization, PluginRegistryEnrollmentAuthorizationError>;
}
