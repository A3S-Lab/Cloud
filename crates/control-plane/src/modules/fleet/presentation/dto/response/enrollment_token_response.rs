use crate::modules::fleet::application::IssueEnrollmentTokenResult;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollmentTokenResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub replayed: bool,
}

impl From<IssueEnrollmentTokenResult> for EnrollmentTokenResponse {
    fn from(result: IssueEnrollmentTokenResult) -> Self {
        let token = result.enrollment_token;
        Self {
            id: token.id.as_uuid(),
            organization_id: token.organization_id.as_uuid(),
            name: token.name,
            aggregate_version: token.aggregate_version,
            created_at: token.created_at,
            expires_at: token.expires_at,
            used_at: token.used_at,
            revoked_at: token.revoked_at,
            replayed: result.replayed,
        }
    }
}
