use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::sources::domain::{
    GithubConnectionLifecycleAcceptance, VerifiedGithubConnectionLifecycle,
};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ReconcileGithubConnectionLifecycle {
    pub lifecycle: VerifiedGithubConnectionLifecycle,
    pub received_at: DateTime<Utc>,
    pub request_id: Uuid,
}

impl Command for ReconcileGithubConnectionLifecycle {
    type Output = ApplicationResult<GithubConnectionLifecycleAcceptance>;
}
