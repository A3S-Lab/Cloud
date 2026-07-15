use crate::modules::workloads::domain::entities::{OciArtifact, OciArtifactReference};
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum OciArtifactResolutionError {
    #[error("invalid OCI reference: {0}")]
    InvalidReference(String),
    #[error("OCI manifest was not found")]
    NotFound,
    #[error("OCI registry denied access to the manifest")]
    Unauthorized,
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
    ) -> Result<OciArtifact, OciArtifactResolutionError>;
}
