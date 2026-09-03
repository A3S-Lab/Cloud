use crate::modules::durable_cells::application::{
    DurableCellSecretBindingAdmissionRequest, IDurableCellSecretBindingPort,
};
use crate::modules::secrets::application::IExactSecretVersionAccess;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use async_trait::async_trait;
use std::sync::Arc;

/// Anti-corruption adapter from the Secrets owner to Durable Cells' exact
/// provider-binding admission port. The canonical materializable-version
/// query keeps scope and active-state checks in one owner transaction.
#[derive(Clone)]
pub struct SecretsDurableCellBindingAdapter {
    secrets: Arc<dyn IExactSecretVersionAccess>,
}

impl SecretsDurableCellBindingAdapter {
    pub fn new(secrets: Arc<dyn IExactSecretVersionAccess>) -> Self {
        Self { secrets }
    }
}

#[async_trait]
impl IDurableCellSecretBindingPort for SecretsDurableCellBindingAdapter {
    async fn validate_active_bindings(
        &self,
        request: &DurableCellSecretBindingAdmissionRequest,
    ) -> ApplicationResult<()> {
        request.validate().map_err(ApplicationError::Invalid)?;
        for binding in &request.bindings {
            self.secrets
                .require_reference(
                    request.organization_id,
                    request.project_id,
                    request.environment_id,
                    *binding,
                )
                .await
                .map_err(binding_error)?;
        }
        Ok(())
    }
}

fn binding_error(error: ApplicationError) -> ApplicationError {
    match error {
        ApplicationError::Forbidden(_) | ApplicationError::NotFound(_) => ApplicationError::Invalid(
            "Durable Cell Secret binding does not reference an active version in this environment"
                .into(),
        ),
        other => other,
    }
}
