use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateMembershipInvitationRequest {
    pub principal_id: Uuid,
    pub role: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MembershipInvitationVersionRequest {
    pub expected_version: u64,
}
