use crate::modules::artifacts::domain::{BuildEvidence, BuildRun};
use crate::modules::sources::domain::ExternalSourceRevision;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildEvidenceGenerationError {
    #[error("build evidence request is invalid: {0}")]
    Invalid(String),
    #[error("build evidence input failed integrity validation: {0}")]
    Integrity(String),
    #[error("build evidence dependency is temporarily unavailable: {0}")]
    Unavailable(String),
    #[error("build evidence storage failed: {0}")]
    Storage(String),
}

#[async_trait]
pub trait IBuildEvidenceGenerator: Send + Sync {
    async fn generate(
        &self,
        build: &BuildRun,
        revision: &ExternalSourceRevision,
        attested_at: DateTime<Utc>,
    ) -> Result<BuildEvidence, BuildEvidenceGenerationError>;
}
