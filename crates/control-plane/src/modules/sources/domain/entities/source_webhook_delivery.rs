use crate::modules::shared_kernel::domain::canonical_timestamp;
use crate::modules::sources::domain::value_objects::{
    GitCommitSha, GitProvider, GitReference, GitRepository, GithubInstallationId,
    PullRequestChangeKind, WebhookDeliveryId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceWebhookDelivery {
    pub provider: GitProvider,
    pub delivery_id: WebhookDeliveryId,
    pub installation_id: GithubInstallationId,
    pub repository: GitRepository,
    pub payload: SourceWebhookPayload,
    pub payload_digest: String,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SourceWebhookPayload {
    Push(SourcePushWebhookDelivery),
    PullRequest(SourcePullRequestWebhookDelivery),
}

impl SourceWebhookPayload {
    pub const fn event_kind(&self) -> &'static str {
        match self {
            Self::Push(_) => "push",
            Self::PullRequest(_) => "pull_request",
        }
    }

    pub fn branch(&self) -> &GitReference {
        match self {
            Self::Push(push) => &push.reference,
            Self::PullRequest(change) => &change.base_reference,
        }
    }

    pub fn commit_sha(&self) -> &GitCommitSha {
        match self {
            Self::Push(push) => &push.commit_sha,
            Self::PullRequest(change) => &change.head_commit_sha,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePushWebhookDelivery {
    pub reference: GitReference,
    pub commit_sha: GitCommitSha,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePullRequestWebhookDelivery {
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
}

#[derive(Debug, Clone)]
pub struct NewSourceWebhookDelivery {
    pub provider: GitProvider,
    pub delivery_id: WebhookDeliveryId,
    pub installation_id: GithubInstallationId,
    pub repository: GitRepository,
    pub payload: SourceWebhookPayload,
    pub payload_digest: String,
    pub received_at: DateTime<Utc>,
}

impl SourceWebhookDelivery {
    pub fn accept(input: NewSourceWebhookDelivery) -> Result<Self, String> {
        Self::restore(Self {
            provider: input.provider,
            delivery_id: input.delivery_id,
            installation_id: input.installation_id,
            repository: input.repository,
            payload: input.payload,
            payload_digest: input.payload_digest,
            received_at: input.received_at,
        })
    }

    pub fn restore(mut delivery: Self) -> Result<Self, String> {
        if delivery.provider != delivery.repository.provider() {
            return Err("source webhook provider does not match its repository".into());
        }
        let repository = GitRepository::parse(
            delivery.repository.provider(),
            delivery.repository.canonical_url(),
        )?;
        if repository != delivery.repository {
            return Err("source webhook repository is not canonical".into());
        }
        if !is_sha256_digest(&delivery.payload_digest) {
            return Err("source webhook payload digest must be a lowercase SHA-256 digest".into());
        }
        match &delivery.payload {
            SourceWebhookPayload::Push(push) => validate_push(push)?,
            SourceWebhookPayload::PullRequest(change) => {
                validate_pull_request(delivery.provider, change)?
            }
        }
        delivery.received_at = canonical_timestamp(delivery.received_at);
        Ok(delivery)
    }

    pub fn same_payload_as(&self, other: &Self) -> bool {
        self.provider == other.provider
            && self.delivery_id == other.delivery_id
            && self.installation_id == other.installation_id
            && self.repository == other.repository
            && self.payload == other.payload
            && self.payload_digest == other.payload_digest
    }
}

fn validate_push(push: &SourcePushWebhookDelivery) -> Result<(), String> {
    if !matches!(push.reference, GitReference::Branch(_)) {
        return Err("source push webhook must contain a branch reference".into());
    }
    if push.commit_sha.as_str().bytes().all(|byte| byte == b'0') {
        return Err("source push webhook cannot contain the deletion sentinel".into());
    }
    GitReference::parse(push.reference.kind(), push.reference.value().to_owned())?;
    GitCommitSha::parse(push.commit_sha.as_str())?;
    Ok(())
}

fn validate_pull_request(
    provider: GitProvider,
    change: &SourcePullRequestWebhookDelivery,
) -> Result<(), String> {
    if change.pull_request_id == 0
        || change.pull_request_id > i64::MAX as u64
        || change.pull_request_number == 0
        || change.pull_request_number > i64::MAX as u64
        || change
            .head_repository
            .as_ref()
            .is_some_and(|repository| repository.provider() != provider)
        || !matches!(change.base_reference, GitReference::Branch(_))
        || !matches!(change.head_reference, GitReference::Branch(_))
        || change
            .head_commit_sha
            .as_str()
            .bytes()
            .all(|byte| byte == b'0')
        || change.provider_created_at != canonical_timestamp(change.provider_created_at)
        || change.provider_updated_at != canonical_timestamp(change.provider_updated_at)
        || change.provider_created_at > change.provider_updated_at
        || !change.kind.is_terminal() && change.merged
        || !change.kind.is_terminal() && change.head_repository.is_none()
    {
        return Err("source pull-request webhook identity or state is invalid".into());
    }
    if let Some(repository) = &change.head_repository {
        let canonical = GitRepository::parse(repository.provider(), repository.canonical_url())?;
        if &canonical != repository {
            return Err("source pull-request head repository is not canonical".into());
        }
    }
    GitReference::parse(
        change.base_reference.kind(),
        change.base_reference.value().to_owned(),
    )?;
    GitReference::parse(
        change.head_reference.kind(),
        change.head_reference.value().to_owned(),
    )?;
    GitCommitSha::parse(change.head_commit_sha.as_str())?;
    Ok(())
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}
