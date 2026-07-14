use super::ApiTokenResponse;
use crate::modules::identity::application::commands::bootstrap_identity::BootstrapIdentityResult;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapIdentityResponse {
    pub organization: BootstrapOrganizationResponse,
    pub api_token: ApiTokenResponse,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapOrganizationResponse {
    pub id: Uuid,
    pub name: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
}

impl From<BootstrapIdentityResult> for BootstrapIdentityResponse {
    fn from(result: BootstrapIdentityResult) -> Self {
        let organization = result.identity.organization;
        Self {
            organization: BootstrapOrganizationResponse {
                id: organization.id.as_uuid(),
                name: organization.name.as_str().to_owned(),
                aggregate_version: organization.aggregate_version,
                created_at: organization.created_at,
            },
            api_token: ApiTokenResponse::new(result.identity.api_token, result.replayed),
            replayed: result.replayed,
        }
    }
}
