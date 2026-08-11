use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateApiTokenRequest {
    pub name: String,
    pub token: String,
    pub scopes: Vec<String>,
    pub principal_id: Option<Uuid>,
    pub expires_at: Option<DateTime<Utc>>,
}
