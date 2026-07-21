use crate::modules::sources::domain::{
    GitCommitSha, GitReference, GitRepository, SourceProviderCredential,
};
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceResolutionRequest {
    pub repository: GitRepository,
    pub reference: GitReference,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSource {
    pub repository: GitRepository,
    pub commit_sha: GitCommitSha,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceResolutionError {
    #[error("source repository or reference is unavailable")]
    Unavailable,
    #[error("source provider is unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("source provider returned an invalid response: {0}")]
    Protocol(String),
}

#[async_trait]
pub trait ISourceResolver: Send + Sync {
    async fn resolve(
        &self,
        request: &SourceResolutionRequest,
        credential: Option<&SourceProviderCredential>,
    ) -> Result<ResolvedSource, SourceResolutionError>;
}
