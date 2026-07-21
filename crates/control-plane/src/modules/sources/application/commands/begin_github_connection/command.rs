use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Command;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct BeginGithubConnection {
    pub organization_id: OrganizationId,
    pub requested_at: DateTime<Utc>,
}

impl Command for BeginGithubConnection {
    type Output = ApplicationResult<BeginGithubConnectionResult>;
}

pub struct BeginGithubConnectionResult {
    pub installation_url: String,
    pub expires_at: DateTime<Utc>,
}
