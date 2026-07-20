use crate::modules::sources::domain::{
    GitCommitSha, GitProvider, GitReference, GitRepository, GithubInstallationId, WebhookDeliveryId,
};

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
pub enum VerifiedSourceWebhook {
    Ignored,
    Push(VerifiedSourcePush),
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
