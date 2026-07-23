use crate::modules::artifacts::domain::BuildRun;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{BuildRunId, OrganizationId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct RetryBuildRun {
    pub organization_id: OrganizationId,
    pub build_run_id: BuildRunId,
    pub idempotency_key: String,
    pub requested_at: DateTime<Utc>,
}

impl Command for RetryBuildRun {
    type Output = ApplicationResult<RetryBuildRunResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RetryBuildRunResult {
    pub build_run: BuildRun,
    pub retry_of_build_run_id: BuildRunId,
    pub replayed: bool,
}
