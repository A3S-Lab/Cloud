use crate::modules::artifacts::domain::{
    BuildArtifact, ValidatedBuildCache, ValidatedOciBuildOutput,
};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRuntimeBuildOutput {
    pub output: ValidatedOciBuildOutput,
    pub cache: Option<ValidatedBuildCache>,
}

#[async_trait]
pub trait IBuildOutputValidator: Send + Sync {
    async fn validate(
        &self,
        artifact: &BuildArtifact,
        recipe: &BuildRecipe,
        expected_cache_key: Option<&str>,
    ) -> Result<ValidatedRuntimeBuildOutput, BuildOutputValidationError>;
}
