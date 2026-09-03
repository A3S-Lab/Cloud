use crate::modules::data::{
    ObjectNamespaceCredentialAdmission, ObjectNamespaceCredentialBinding,
    ObjectNamespaceCredentialBindingSpec,
};
use crate::modules::durable_cells::application::{
    DurableCellStorageCredentialRequest, IDurableCellStoragePort,
};
use crate::modules::secrets::domain::ISecretRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use async_trait::async_trait;
use std::sync::Arc;

/// Anti-corruption adapter from Durable Cells' exact S0 credential identity to
/// Data's credential admission service. Data reconstructs its digest-locked
/// binding and Secrets remains the only active-version authority.
#[derive(Clone)]
pub struct DataDurableCellStorageAdapter {
    credential_admission: ObjectNamespaceCredentialAdmission,
}

impl DataDurableCellStorageAdapter {
    pub fn new(secrets: Arc<dyn ISecretRepository>) -> Self {
        Self {
            credential_admission: ObjectNamespaceCredentialAdmission::new(secrets),
        }
    }
}

#[async_trait]
impl IDurableCellStoragePort for DataDurableCellStorageAdapter {
    async fn require_active_credentials(
        &self,
        request: &DurableCellStorageCredentialRequest,
    ) -> ApplicationResult<()> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let binding = ObjectNamespaceCredentialBinding::restore(
            ObjectNamespaceCredentialBindingSpec {
                organization_id: request.organization_id,
                project_id: request.project_id,
                environment_id: request.environment_id,
                namespace_id: request.namespace_id,
                generation: request.generation,
                provider_profile_digest: request.provider_profile_digest.clone(),
                access_key_id: request.access_key_id,
                secret_access_key: request.secret_access_key,
                session_token: request.session_token,
            },
            request.binding_digest.as_str(),
        )
        .map_err(|error| {
            ApplicationError::Internal(format!(
                "Durable Cell S0 credential projection changed at the Data boundary: {error}"
            ))
        })?;
        self.credential_admission.require_active(&binding).await
    }
}
