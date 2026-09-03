use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, SecretVersionReference,
};
use async_trait::async_trait;

const MAX_SECRET_BINDINGS: usize = 128;

/// Exact, plaintext-free Secret references that a Durable Cell provider
/// template exposes to one environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellSecretBindingAdmissionRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub bindings: Vec<SecretVersionReference>,
}

impl DurableCellSecretBindingAdmissionRequest {
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        bindings: Vec<SecretVersionReference>,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            environment_id,
            bindings,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.bindings.len() > MAX_SECRET_BINDINGS
        {
            return Err("Durable Cell Secret binding admission identity is invalid".into());
        }
        for binding in &self.bindings {
            binding.validate()?;
        }
        Ok(())
    }
}

/// Durable Cells' consumer-owned boundary for checking exact active Secret
/// versions. Secrets remains the sole scope, state, and materialization
/// authority; no plaintext or Secrets aggregate crosses this interface.
#[async_trait]
pub trait IDurableCellSecretBindingPort: Send + Sync {
    async fn validate_active_bindings(
        &self,
        request: &DurableCellSecretBindingAdmissionRequest,
    ) -> ApplicationResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::SecretId;
    use uuid::Uuid;

    #[test]
    fn admission_request_accepts_an_empty_binding_set() {
        let request = DurableCellSecretBindingAdmissionRequest::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            Vec::new(),
        );
        request.validate().expect("valid empty binding set");
    }

    #[test]
    fn admission_request_rejects_an_invalid_reference() {
        let request = DurableCellSecretBindingAdmissionRequest::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            vec![SecretVersionReference {
                secret_id: SecretId::from_uuid(Uuid::nil()),
                version: 1,
            }],
        );
        assert!(request.validate().is_err());
    }
}
