use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BootstrapIdentityRequest {
    pub organization_name: String,
    pub token_name: String,
    pub token: String,
    pub expires_at: Option<DateTime<Utc>>,
}
