use crate::modules::shared_kernel::domain::canonical_timestamp;
use crate::modules::sources::domain::value_objects::{
    GitCommitSha, GitProvider, GitReference, GitRepository, GithubInstallationId, WebhookDeliveryId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceWebhookDelivery {
    pub provider: GitProvider,
    pub delivery_id: WebhookDeliveryId,
    pub repository: GitRepository,
    pub installation_id: GithubInstallationId,
    pub reference: GitReference,
    pub commit_sha: GitCommitSha,
    pub payload_digest: String,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewSourceWebhookDelivery {
    pub provider: GitProvider,
    pub delivery_id: WebhookDeliveryId,
    pub repository: GitRepository,
    pub installation_id: GithubInstallationId,
    pub reference: GitReference,
    pub commit_sha: GitCommitSha,
    pub payload_digest: String,
    pub received_at: DateTime<Utc>,
}

impl SourceWebhookDelivery {
    pub fn accept(input: NewSourceWebhookDelivery) -> Result<Self, String> {
        Self::restore(Self {
            provider: input.provider,
            delivery_id: input.delivery_id,
            repository: input.repository,
            installation_id: input.installation_id,
            reference: input.reference,
            commit_sha: input.commit_sha,
            payload_digest: input.payload_digest,
            received_at: input.received_at,
        })
    }

    pub fn restore(mut delivery: Self) -> Result<Self, String> {
        if delivery.provider != delivery.repository.provider() {
            return Err("source webhook provider does not match its repository".into());
        }
        if !matches!(delivery.reference, GitReference::Branch(_)) {
            return Err("source push webhook must contain a branch reference".into());
        }
        if delivery
            .commit_sha
            .as_str()
            .bytes()
            .all(|byte| byte == b'0')
        {
            return Err("source push webhook cannot contain the deletion sentinel".into());
        }
        if !is_sha256_digest(&delivery.payload_digest) {
            return Err("source webhook payload digest must be a lowercase SHA-256 digest".into());
        }
        delivery.received_at = canonical_timestamp(delivery.received_at);
        Ok(delivery)
    }

    pub fn same_payload_as(&self, other: &Self) -> bool {
        self.provider == other.provider
            && self.delivery_id == other.delivery_id
            && self.repository == other.repository
            && self.installation_id == other.installation_id
            && self.reference == other.reference
            && self.commit_sha == other.commit_sha
            && self.payload_digest == other.payload_digest
    }
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}
