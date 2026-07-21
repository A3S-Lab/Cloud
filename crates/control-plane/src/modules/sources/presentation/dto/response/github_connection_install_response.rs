use crate::modules::sources::BeginGithubConnectionResult;
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubConnectionInstallResponse {
    pub provider: &'static str,
    pub installation_url: String,
    pub expires_at: DateTime<Utc>,
}

impl From<BeginGithubConnectionResult> for GithubConnectionInstallResponse {
    fn from(result: BeginGithubConnectionResult) -> Self {
        Self {
            provider: "github",
            installation_url: result.installation_url,
            expires_at: result.expires_at,
        }
    }
}
