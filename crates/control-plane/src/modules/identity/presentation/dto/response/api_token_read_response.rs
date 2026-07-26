use crate::modules::identity::domain::entities::ApiToken;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTokenReadResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<ApiToken> for ApiTokenReadResponse {
    fn from(token: ApiToken) -> Self {
        Self {
            id: token.id.as_uuid(),
            organization_id: token.organization_id.as_uuid(),
            name: token.name.as_str().to_owned(),
            scopes: token
                .scopes
                .iter()
                .map(|scope| scope.as_str().to_owned())
                .collect(),
            aggregate_version: token.aggregate_version,
            created_at: token.created_at,
            expires_at: token.expires_at,
            revoked_at: token.revoked_at,
        }
    }
}
