use crate::modules::artifacts::domain::{BuildRun, BuildSource};
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildSourceResolutionError {
    #[error("build source request is invalid: {0}")]
    Invalid(String),
    #[error("build source identity conflicts with durable state")]
    Conflict,
    #[error("build source was not found")]
    NotFound,
    #[error("build source is temporarily unavailable: {0}")]
    Unavailable(String),
    #[error("build source failed integrity validation: {0}")]
    Integrity(String),
    #[error("build source storage failed: {0}")]
    Storage(String),
}

#[async_trait]
pub trait IBuildSourceResolver: Send + Sync {
    async fn resolve(&self, build: &BuildRun) -> Result<BuildSource, BuildSourceResolutionError>;
}
