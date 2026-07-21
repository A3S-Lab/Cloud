use crate::modules::sources::domain::{GitCommitSha, GitRepository, SourceProviderCredential};
use async_trait::async_trait;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCheckoutRequest {
    pub checkout_id: Uuid,
    pub repository: GitRepository,
    pub commit_sha: GitCommitSha,
}

impl SourceCheckoutRequest {
    pub fn new(
        checkout_id: Uuid,
        repository: GitRepository,
        commit_sha: GitCommitSha,
    ) -> Result<Self, String> {
        if checkout_id.is_nil() {
            return Err("source checkout ID cannot be nil".into());
        }
        Ok(Self {
            checkout_id,
            repository,
            commit_sha,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedOutSource {
    pub checkout_id: Uuid,
    pub repository: GitRepository,
    pub commit_sha: GitCommitSha,
    pub directory: PathBuf,
    pub git_tree_id: String,
    pub content_digest: String,
    pub file_count: usize,
    pub content_bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceCheckoutError {
    #[error("source checkout request is invalid: {0}")]
    Invalid(String),
    #[error("source checkout identity conflicts with an existing checkout")]
    Conflict,
    #[error("source checkout is unavailable: {0}")]
    Unavailable(String),
    #[error("source checkout failed integrity validation: {0}")]
    Integrity(String),
    #[error("source checkout storage failed: {0}")]
    Storage(String),
}

#[async_trait]
pub trait ISourceCheckout: Send + Sync {
    async fn checkout(
        &self,
        request: &SourceCheckoutRequest,
        credential: Option<&SourceProviderCredential>,
    ) -> Result<CheckedOutSource, SourceCheckoutError>;

    async fn remove(&self, checkout_id: Uuid) -> Result<(), SourceCheckoutError>;
}
