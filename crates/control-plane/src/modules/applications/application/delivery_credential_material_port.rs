//! Port for Identity/Secrets mint of anonymous delivery credential material.
//!
//! Applications owns the C16 binding and C20 lifecycle. Plaintext material and
//! exact Secret version writes stay behind this port so Applications never
//! becomes a second Secrets or Identity store.

use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    ApplicationDeliveryCredentialId, ApplicationId, OrganizationId, PrincipalId, ProjectId,
    SecretVersionReference,
};
use async_trait::async_trait;
use zeroize::Zeroizing;

/// One-time minted delivery credential material before C20 register.
#[derive(Clone)]
pub struct ApplicationDeliveryCredentialMaterial {
    pub lookup_key: String,
    pub secret: SecretVersionReference,
    pub plaintext_secret: Zeroizing<String>,
}

impl std::fmt::Debug for ApplicationDeliveryCredentialMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApplicationDeliveryCredentialMaterial")
            .field("lookup_key", &self.lookup_key)
            .field("secret", &self.secret)
            .field("plaintext_secret", &"<redacted>")
            .finish()
    }
}

/// Mint opaque lookup key plus exact Secrets version material for C20.
#[async_trait]
pub trait IApplicationDeliveryCredentialMaterialPort: Send + Sync {
    async fn mint(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        credential_id: ApplicationDeliveryCredentialId,
        actor_principal_id: PrincipalId,
    ) -> ApplicationResult<ApplicationDeliveryCredentialMaterial>;
}
