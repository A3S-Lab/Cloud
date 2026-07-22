use crate::modules::sources::domain::GithubConnection;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubConnectionResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub provider: &'static str,
    pub installation_id: u64,
    pub account: GithubAccountResponse,
    pub verified_by: GithubUserResponse,
    pub status: String,
    pub provider_authority: GithubProviderAuthorityResponse,
    pub connected_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubAccountResponse {
    pub id: u64,
    pub login: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubUserResponse {
    pub id: u64,
    pub login: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubProviderAuthorityResponse {
    pub checked_at: DateTime<Utc>,
    pub check_attempted_at: DateTime<Utc>,
    pub next_check_at: DateTime<Utc>,
    pub consecutive_failures: u32,
    pub last_error: Option<String>,
}

impl From<GithubConnection> for GithubConnectionResponse {
    fn from(connection: GithubConnection) -> Self {
        Self {
            id: connection.id.as_uuid(),
            organization_id: connection.organization_id.as_uuid(),
            provider: "github",
            installation_id: connection.installation_id.as_u64(),
            account: GithubAccountResponse {
                id: connection.account_id.as_u64(),
                login: connection.account_login.as_str().into(),
                kind: connection.account_kind.as_str().into(),
            },
            verified_by: GithubUserResponse {
                id: connection.verified_by_user_id.as_u64(),
                login: connection.verified_by_user_login.as_str().into(),
            },
            status: connection.status.as_str().into(),
            provider_authority: GithubProviderAuthorityResponse {
                checked_at: connection.provider_checked_at,
                check_attempted_at: connection.provider_check_attempted_at,
                next_check_at: connection.provider_next_check_at,
                consecutive_failures: connection.provider_check_failures,
                last_error: connection
                    .provider_check_error
                    .map(|error| error.as_str().into()),
            },
            connected_at: connection.connected_at,
            updated_at: connection.updated_at,
        }
    }
}
