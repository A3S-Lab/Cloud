use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, PrincipalId, ProjectId, PullRequestPreviewId,
    PullRequestPreviewPolicyRevisionId, SourcePullRequestChangeId, SourceSubscriptionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_EVENT_KEY: &str =
    "developer.pull-request-preview.lifecycle-committed";
pub const PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_SCHEMA_VERSION: u32 = 1;

/// Immutable Developer Workflows Published Language for one committed Preview
/// aggregate version. Consumers translate it into their own commands; they do
/// not read the Preview row, policy aggregate, or projection receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PullRequestPreviewLifecycleCommitted {
    pub source_pull_request_change_id: SourcePullRequestChangeId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub source_environment_id: EnvironmentId,
    pub source_subscription_id: SourceSubscriptionId,
    pub preview_policy_revision_id: PullRequestPreviewPolicyRevisionId,
    pub preview_policy_revision_number: u64,
    pub preview_policy_accepted_at: DateTime<Utc>,
    pub preview_id: PullRequestPreviewId,
    pub preview_aggregate_version: u64,
    pub environment_id: EnvironmentId,
    pub environment_name: String,
    pub owner_principal_id: PrincipalId,
    pub installation_id: u64,
    pub base_repository_provider: String,
    pub base_repository_url: String,
    pub base_repository_identity: String,
    pub base_branch: String,
    pub head_repository_provider: Option<String>,
    pub head_repository_url: Option<String>,
    pub head_repository_identity: Option<String>,
    pub head_branch: String,
    pub head_commit_sha: String,
    pub pull_request_id: u64,
    pub pull_request_number: u64,
    pub provider_created_at: DateTime<Utc>,
    pub last_provider_updated_at: DateTime<Utc>,
    pub last_change_kind: String,
    pub last_merged: bool,
    pub expires_at: DateTime<Utc>,
    pub status: String,
    pub cleanup_reason: Option<String>,
    pub cleanup_requested_at: Option<DateTime<Utc>>,
    pub fork_policy: String,
    pub is_fork: bool,
    pub allow_protected_secrets_for_trusted_sources: bool,
    pub protected_secrets_eligible: bool,
    pub lifetime_seconds: u32,
    pub maximum_active_previews: u16,
    pub maximum_workloads: u16,
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub ephemeral_storage_bytes: u64,
}

impl PullRequestPreviewLifecycleCommitted {
    pub fn is_active(&self) -> bool {
        self.status == "active"
    }
}
