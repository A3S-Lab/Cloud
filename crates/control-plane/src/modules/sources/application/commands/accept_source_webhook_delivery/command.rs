use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::sources::domain::{SourceWebhookDelivery, VerifiedRepositoryWebhook};
use crate::modules::sources::published::PullRequestChangeCommittedFact;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AcceptSourceWebhookDelivery {
    pub webhook: VerifiedRepositoryWebhook,
    pub received_at: DateTime<Utc>,
    pub request_id: Uuid,
}

impl Command for AcceptSourceWebhookDelivery {
    type Output = ApplicationResult<AcceptSourceWebhookDeliveryResult>;
}

#[derive(Debug, Clone, Serialize)]
pub struct AcceptSourceWebhookDeliveryResult {
    pub delivery: SourceWebhookDelivery,
    pub replayed: bool,
    pub revisions: Vec<crate::modules::sources::domain::ExternalSourceRevision>,
    pub pull_request_changes: Vec<PullRequestChangeCommittedFact>,
}
