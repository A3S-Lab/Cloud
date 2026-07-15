use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IssueEnrollmentTokenRequest {
    pub name: String,
    pub token: String,
    pub expires_at: DateTime<Utc>,
}
