use crate::modules::identity::domain::entities::IdentityBootstrap;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone)]
pub struct BootstrapIdentity {
    pub organization_name: String,
    pub token_name: String,
    pub token_secret: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for BootstrapIdentity {
    type Output = ApplicationResult<BootstrapIdentityResult>;
}

#[derive(Debug, Clone, Serialize)]
pub struct BootstrapIdentityResult {
    pub identity: IdentityBootstrap,
    pub replayed: bool,
}
