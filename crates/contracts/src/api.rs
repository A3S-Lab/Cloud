use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiSuccessResponse<T> {
    pub code: u16,
    pub message: String,
    pub data: T,
    pub request_id: Uuid,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiErrorResponse {
    pub code: u16,
    pub status_code: String,
    pub message: String,
    pub details: Value,
    pub request_id: Uuid,
    pub timestamp: DateTime<Utc>,
}
