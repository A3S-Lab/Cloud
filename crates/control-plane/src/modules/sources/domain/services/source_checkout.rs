use crate::modules::shared_kernel::domain::Sha256Digest;
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

    pub fn validate(&self) -> Result<(), String> {
        if self.checkout_id.is_nil()
            || GitRepository::parse(self.repository.provider(), self.repository.canonical_url())?
                != self.repository
            || GitCommitSha::parse(self.commit_sha.as_str())? != self.commit_sha
        {
            return Err("source checkout request is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckedOutSourceEntryKind {
    Regular,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedOutSourceEntry {
    path: String,
    kind: CheckedOutSourceEntryKind,
    size_bytes: u64,
    content_digest: Sha256Digest,
}

impl CheckedOutSourceEntry {
    pub fn new(
        path: impl Into<String>,
        kind: CheckedOutSourceEntryKind,
        size_bytes: u64,
        content_digest: Sha256Digest,
    ) -> Result<Self, String> {
        let value = Self {
            path: path.into(),
            kind,
            size_bytes,
            content_digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_checked_out_source_path(&self.path)?;
        if Sha256Digest::parse(self.content_digest.as_str())? != self.content_digest {
            return Err("checked-out source entry digest is not canonical".into());
        }
        Ok(())
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub const fn kind(&self) -> CheckedOutSourceEntryKind {
        self.kind
    }

    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    pub const fn content_digest(&self) -> &Sha256Digest {
        &self.content_digest
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
    pub entries: Vec<CheckedOutSourceEntry>,
}

impl CheckedOutSource {
    pub fn validate_for(&self, request: &SourceCheckoutRequest) -> Result<(), String> {
        request.validate()?;
        if self.checkout_id != request.checkout_id
            || self.repository != request.repository
            || self.commit_sha != request.commit_sha
            || self.directory.as_os_str().is_empty()
            || GitCommitSha::parse(self.git_tree_id.as_str())?.as_str() != self.git_tree_id.as_str()
            || Sha256Digest::parse(self.content_digest.as_str())?.as_str()
                != self.content_digest.as_str()
            || self.file_count != self.entries.len()
        {
            return Err("source checkout receipt differs from its exact request".into());
        }

        let mut content_bytes = 0_u64;
        let mut previous: Option<&str> = None;
        for entry in &self.entries {
            entry.validate()?;
            if previous.is_some_and(|path| path >= entry.path()) {
                return Err("checked-out source entries are not canonical".into());
            }
            previous = Some(entry.path());
            content_bytes = content_bytes
                .checked_add(entry.size_bytes())
                .ok_or_else(|| "checked-out source content size overflowed".to_owned())?;
        }
        if content_bytes != self.content_bytes {
            return Err("checked-out source entry bytes differ from its receipt".into());
        }
        Ok(())
    }
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

    /// Revalidate one already committed checkout without provider access or
    /// recreating missing bytes.
    async fn replay(
        &self,
        request: &SourceCheckoutRequest,
    ) -> Result<CheckedOutSource, SourceCheckoutError>;

    async fn remove(&self, checkout_id: Uuid) -> Result<(), SourceCheckoutError>;
}

pub(crate) fn validate_checked_out_source_path(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 4096
        || value.starts_with('/')
        || value.starts_with("./")
        || value.contains(['\\', '\0'])
        || value.chars().any(char::is_control)
    {
        return Err("checked-out source path must be a canonical relative POSIX path".into());
    }
    for segment in value.split('/') {
        if segment.is_empty()
            || matches!(segment, "." | "..")
            || segment.eq_ignore_ascii_case(".git")
        {
            return Err("checked-out source path contains an unsafe segment".into());
        }
    }
    Ok(())
}
