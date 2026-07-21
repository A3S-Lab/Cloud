use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use zeroize::Zeroizing;

pub struct PrepareGithubConnectionOauth {
    pub installation_id: u64,
    pub installation_state: Zeroizing<String>,
    pub requested_at: DateTime<Utc>,
}

impl Command for PrepareGithubConnectionOauth {
    type Output = ApplicationResult<PrepareGithubConnectionOauthResult>;
}

pub struct PrepareGithubConnectionOauthResult {
    pub authorization_url: String,
    pub pkce_verifier: Zeroizing<String>,
    pub expires_at: DateTime<Utc>,
}
