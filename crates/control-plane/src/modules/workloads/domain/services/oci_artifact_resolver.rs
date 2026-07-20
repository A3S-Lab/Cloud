use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId, SecretId};
use crate::modules::workloads::domain::entities::{OciArtifact, OciArtifactReference};
use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OciRegistryCredentialReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub secret_id: SecretId,
    pub version: u64,
}

impl OciRegistryCredentialReference {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.secret_id.as_uuid().is_nil()
            || self.version == 0
        {
            return Err("OCI registry credential reference is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OciArtifactResolutionError {
    #[error("invalid OCI reference: {0}")]
    InvalidReference(String),
    #[error("OCI manifest was not found")]
    NotFound,
    #[error("OCI registry denied access to the manifest")]
    Unauthorized,
    #[error("OCI registry credential is unavailable: {0}")]
    Credential(String),
    #[error("OCI registry request failed: {0}")]
    Registry(String),
    #[error("OCI registry returned an invalid manifest response: {0}")]
    Protocol(String),
}

#[async_trait]
pub trait IOciArtifactResolver: Send + Sync {
    async fn resolve(
        &self,
        reference: &OciArtifactReference,
        registry_credential: Option<&OciRegistryCredentialReference>,
    ) -> Result<OciArtifact, OciArtifactResolutionError>;
}
