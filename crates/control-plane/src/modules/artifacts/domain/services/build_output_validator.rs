use crate::modules::artifacts::domain::{BuildArtifact, ValidatedOciBuildOutput};
use crate::modules::sources::domain::BuildRecipe;
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildOutputValidationError {
    #[error("build output request is invalid: {0}")]
    Invalid(String),
    #[error("build output is temporarily unavailable: {0}")]
    Unavailable(String),
    #[error("build output failed integrity validation: {0}")]
    Integrity(String),
    #[error("build output storage failed: {0}")]
    Storage(String),
}

#[async_trait]
pub trait IBuildOutputValidator: Send + Sync {
    async fn validate(
        &self,
        artifact: &BuildArtifact,
        recipe: &BuildRecipe,
    ) -> Result<ValidatedOciBuildOutput, BuildOutputValidationError>;
}
