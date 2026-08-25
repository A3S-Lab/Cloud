use crate::modules::shared_kernel::domain::canonical_timestamp;
use crate::modules::sources::domain::{
    GitCommitSha, GitProvider, GitReference, GitRepository, GithubConnectionLifecycleChange,
    GithubInstallationId, PullRequestChangeKind, WebhookDeliveryId,
};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPullRequestChange {
    pub provider: GitProvider,
    pub delivery_id: WebhookDeliveryId,
    pub installation_id: GithubInstallationId,
    pub base_repository: GitRepository,
    pub base_reference: GitReference,
    pub head_repository: Option<GitRepository>,
    pub head_reference: GitReference,
    pub head_commit_sha: GitCommitSha,
    pub pull_request_id: u64,
    pub pull_request_number: u64,
    pub kind: PullRequestChangeKind,
    pub merged: bool,
    pub provider_created_at: DateTime<Utc>,
    pub provider_updated_at: DateTime<Utc>,
    pub payload_digest: String,
}

impl VerifiedPullRequestChange {
    pub fn validate(&self) -> Result<(), String> {
        if self.pull_request_id == 0
            || self.pull_request_id > i64::MAX as u64
            || self.pull_request_number == 0
            || self.pull_request_number > i64::MAX as u64
            || self.provider != self.base_repository.provider()
            || self
                .head_repository
                .as_ref()
                .is_some_and(|repository| repository.provider() != self.provider)
            || !matches!(self.base_reference, GitReference::Branch(_))
            || !matches!(self.head_reference, GitReference::Branch(_))
            || self
                .head_commit_sha
                .as_str()
                .bytes()
                .all(|byte| byte == b'0')
            || !is_sha256_digest(&self.payload_digest)
            || self.provider_created_at != canonical_timestamp(self.provider_created_at)
            || self.provider_updated_at != canonical_timestamp(self.provider_updated_at)
            || self.provider_created_at > self.provider_updated_at
            || !self.kind.is_terminal() && self.merged
            || !self.kind.is_terminal() && self.head_repository.is_none()
        {
            return Err("verified pull-request change identity or state is invalid".into());
        }
        let base = GitRepository::parse(
            self.base_repository.provider(),
            self.base_repository.canonical_url(),
        )?;
        if base != self.base_repository {
            return Err("pull-request base repository is not canonical".into());
        }
        if let Some(repository) = &self.head_repository {
            let head = GitRepository::parse(repository.provider(), repository.canonical_url())?;
            if &head != repository {
                return Err("pull-request head repository is not canonical".into());
            }
        }
        GitReference::parse(
            self.base_reference.kind(),
            self.base_reference.value().to_owned(),
        )?;
        GitReference::parse(
            self.head_reference.kind(),
            self.head_reference.value().to_owned(),
        )?;
        GitCommitSha::parse(self.head_commit_sha.as_str())?;
        Ok(())
    }

    pub fn is_fork(&self) -> bool {
        self.head_repository
            .as_ref()
            .is_none_or(|repository| repository != &self.base_repository)
    }
}

#[derive(Debug, Clone)]
pub struct VerifiedSourcePush {
    pub provider: GitProvider,
    pub delivery_id: WebhookDeliveryId,
    pub repository: GitRepository,
    pub installation_id: GithubInstallationId,
    pub reference: GitReference,
    pub commit_sha: GitCommitSha,
    pub payload_digest: String,
}

#[derive(Debug, Clone)]
pub struct VerifiedGithubConnectionLifecycle {
    pub provider: GitProvider,
    pub delivery_id: WebhookDeliveryId,
    pub change: GithubConnectionLifecycleChange,
    pub payload_digest: String,
}

#[derive(Debug, Clone)]
pub enum VerifiedRepositoryWebhook {
    Push(VerifiedSourcePush),
    PullRequest(VerifiedPullRequestChange),
}

impl VerifiedRepositoryWebhook {
    pub const fn installation_id(&self) -> GithubInstallationId {
        match self {
            Self::Push(push) => push.installation_id,
            Self::PullRequest(change) => change.installation_id,
        }
    }
}

#[derive(Debug, Clone)]
pub enum VerifiedSourceWebhook {
    Ignored,
    Repository(VerifiedRepositoryWebhook),
    GithubConnectionLifecycle(VerifiedGithubConnectionLifecycle),
}

#[derive(Debug, Clone, Copy)]
pub struct SourceWebhookVerificationRequest<'a> {
    pub event: &'a str,
    pub delivery_id: &'a str,
    pub signature: &'a str,
    pub body: &'a [u8],
}

#[derive(Debug, thiserror::Error)]
pub enum SourceWebhookVerificationError {
    #[error("source webhook authentication failed")]
    Authentication,
    #[error("source webhook payload exceeds the {maximum_bytes}-byte limit")]
    PayloadTooLarge { maximum_bytes: usize },
    #[error("source webhook payload is invalid: {0}")]
    Invalid(String),
    #[error("source webhook verification is unavailable: {0}")]
    Unavailable(String),
}

pub trait ISourceWebhookVerifier: Send + Sync {
    fn verify(
        &self,
        request: SourceWebhookVerificationRequest<'_>,
    ) -> Result<VerifiedSourceWebhook, SourceWebhookVerificationError>;
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}
