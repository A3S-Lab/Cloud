use crate::modules::identity::domain::entities::ApiToken;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{ApiTokenId, OrganizationId};
use a3s_boot::Command;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RevokeApiToken {
    pub organization_id: OrganizationId,
    pub token_id: ApiTokenId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for RevokeApiToken {
    type Output = ApplicationResult<RevokeApiTokenResult>;
}

#[derive(Debug, Clone, Serialize)]
pub struct RevokeApiTokenResult {
    pub api_token: ApiToken,
    pub replayed: bool,
}
