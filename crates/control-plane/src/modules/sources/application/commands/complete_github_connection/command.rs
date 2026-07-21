use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::sources::domain::GithubConnection;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use zeroize::Zeroizing;

pub struct CompleteGithubConnection {
    pub oauth_state: Zeroizing<String>,
    pub code: Zeroizing<String>,
    pub pkce_verifier: Zeroizing<String>,
    pub request_id: Uuid,
    pub completed_at: DateTime<Utc>,
}

impl Command for CompleteGithubConnection {
    type Output = ApplicationResult<GithubConnection>;
}
