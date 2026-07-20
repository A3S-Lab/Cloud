use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::sources::domain::{SourceWebhookDelivery, VerifiedSourcePush};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct AcceptSourceWebhookDelivery {
    pub push: VerifiedSourcePush,
    pub received_at: DateTime<Utc>,
}

impl Command for AcceptSourceWebhookDelivery {
    type Output = ApplicationResult<AcceptSourceWebhookDeliveryResult>;
}

#[derive(Debug, Clone, Serialize)]
pub struct AcceptSourceWebhookDeliveryResult {
    pub delivery: SourceWebhookDelivery,
    pub replayed: bool,
}
