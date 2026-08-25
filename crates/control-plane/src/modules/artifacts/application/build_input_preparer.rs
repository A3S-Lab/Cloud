use crate::modules::artifacts::domain::{BuildArtifact, BuildRun, BuildSource};
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedBuildInput {
    pub source_content_digest: String,
    pub artifact: BuildArtifact,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildInputPreparationError {
    #[error("build input request is invalid: {0}")]
    Invalid(String),
    #[error("build input identity conflicts with durable source state")]
    Conflict,
    #[error("build input is temporarily unavailable: {0}")]
    Unavailable(String),
    #[error("build input failed integrity validation: {0}")]
    Integrity(String),
    #[error("build input storage failed: {0}")]
    Storage(String),
}

/// Application boundary for materializing immutable bytes for one BuildRun.
///
/// Input preparation performs provider and storage I/O; it is not a domain
/// service. Domain owns the BuildRun and admitted artifact values only.
#[async_trait]
pub trait IBuildInputPreparer: Send + Sync {
    async fn prepare(
        &self,
        build: &BuildRun,
        source: &BuildSource,
    ) -> Result<PreparedBuildInput, BuildInputPreparationError>;

    async fn remove(&self, build: &BuildRun) -> Result<(), BuildInputPreparationError>;
}
