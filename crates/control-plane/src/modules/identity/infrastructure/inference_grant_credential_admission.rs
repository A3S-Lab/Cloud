//! Identity-owned admission for Inference route grant→credential publication.

use crate::modules::identity::domain::repositories::IInferenceCredentialRepository;
use crate::modules::inference::application::{
    IInferenceGrantCredentialAdmissionPort, InferenceGrantCredentialAdmissionRequest,
    INFERENCE_GRANT_CREDENTIAL_INVALID,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::InferenceCredentialId;
use async_trait::async_trait;
use std::sync::Arc;

/// Identity anti-corruption adapter that fail-closes Inference route grants
/// against same-environment active credentials and exact generation match.
#[derive(Clone)]
pub struct IdentityInferenceGrantCredentialAdmissionAdapter {
    credentials: Arc<dyn IInferenceCredentialRepository>,
}

impl IdentityInferenceGrantCredentialAdmissionAdapter {
    pub fn new(credentials: Arc<dyn IInferenceCredentialRepository>) -> Self {
        Self { credentials }
    }
}

#[async_trait]
impl IInferenceGrantCredentialAdmissionPort for IdentityInferenceGrantCredentialAdmissionAdapter {
    async fn admit(
        &self,
        request: InferenceGrantCredentialAdmissionRequest,
    ) -> ApplicationResult<()> {
        for grant in &request.grants {
            if grant.credential_id.is_nil() {
                return Err(grant_invalid(
                    "inference grant credential_id must be non-nil",
                ));
            }
            if grant.credential_generation == 0 {
                return Err(grant_invalid(
                    "inference grant credential_generation must be greater than 0",
                ));
            }

            let credential_id = InferenceCredentialId::from_uuid(grant.credential_id);
            let credential = match self
                .credentials
                .find_inference_credential(request.organization_id, credential_id)
                .await
            {
                Ok(Some(credential)) => credential,
                Ok(None) => {
                    return Err(grant_invalid(
                        "inference grant references an unknown credential",
                    ))
                }
                Err(error) => return Err(error.into()),
            };

            if credential.organization_id != request.organization_id
                || credential.project_id != request.project_id
                || credential.environment_id != request.environment_id
            {
                return Err(grant_invalid(
                    "inference grant credential does not belong to this organization, project, and environment",
                ));
            }

            if !credential.is_active_at(request.observed_at) {
                return Err(grant_invalid(
                    "inference grant credential is not active at the request time",
                ));
            }

            if credential.generation() != grant.credential_generation {
                return Err(grant_invalid(
                    "inference grant credential_generation does not match the live credential",
                ));
            }
        }

        Ok(())
    }
}

fn grant_invalid(detail: impl Into<String>) -> ApplicationError {
    ApplicationError::Invalid(format!(
        "{INFERENCE_GRANT_CREDENTIAL_INVALID}: {}",
        detail.into()
    ))
}
