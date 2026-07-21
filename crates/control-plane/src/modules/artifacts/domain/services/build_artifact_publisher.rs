use crate::modules::artifacts::domain::{
    BuildRun, OciPublicationRequest, OciPublicationTarget, PublishedOciArtifact,
};
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildArtifactPublicationError {
    #[error("OCI publication request is invalid: {0}")]
    Invalid(String),
    #[error("OCI publication credential is unavailable: {0}")]
    Credential(String),
    #[error("OCI registry rejected publication authorization")]
    Unauthorized,
    #[error("OCI registry publication is temporarily unavailable: {0}")]
    Unavailable(String),
    #[error("OCI publication failed integrity validation: {0}")]
    Integrity(String),
    #[error("OCI registry returned an invalid publication response: {0}")]
    Protocol(String),
    #[error("OCI registry publication failed: {0}")]
    Registry(String),
    #[error("OCI publication storage failed: {0}")]
    Storage(String),
}

#[async_trait]
pub trait IBuildArtifactPublisher: Send + Sync {
    fn target_for(
        &self,
        build: &BuildRun,
    ) -> Result<OciPublicationTarget, BuildArtifactPublicationError>;

    async fn find(
        &self,
        request: &OciPublicationRequest,
    ) -> Result<Option<PublishedOciArtifact>, BuildArtifactPublicationError>;

    async fn publish(
        &self,
        request: &OciPublicationRequest,
    ) -> Result<PublishedOciArtifact, BuildArtifactPublicationError>;
}
