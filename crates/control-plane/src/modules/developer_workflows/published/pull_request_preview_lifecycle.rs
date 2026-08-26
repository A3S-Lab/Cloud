use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GitCommitSha, OrganizationId, PrincipalId, ProjectId,
    PullRequestPreviewId, PullRequestPreviewPolicyRevisionId, SourcePullRequestChangeId,
    SourceSubscriptionId,
};
use crate::modules::sources::published::{GitProvider, GitRepository};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_EVENT_KEY: &str =
    "developer.pull-request-preview.lifecycle-committed";
pub const PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_SCHEMA_VERSION: u32 = 1;
pub const PULL_REQUEST_PREVIEW_LIFECYCLE_MAX_BYTES: usize = 16 * 1024;
pub const PREVIEW_MIN_LIFETIME_SECONDS: u32 = 5 * 60;
pub const PREVIEW_MAX_LIFETIME_SECONDS: u32 = 30 * 24 * 60 * 60;
pub const PREVIEW_MAX_ACTIVE_PER_POLICY: u16 = 256;
pub const PREVIEW_MIN_MEMORY_BYTES: u64 = 64 * 1024 * 1024;
pub const PREVIEW_MAX_MEMORY_BYTES: u64 = 512 * 1024 * 1024 * 1024;
pub const PREVIEW_MIN_STORAGE_BYTES: u64 = 64 * 1024 * 1024;
pub const PREVIEW_MAX_STORAGE_BYTES: u64 = 4 * 1024 * 1024 * 1024 * 1024;
pub const PREVIEW_MAX_CPU_MILLIS: u64 = 128_000;
pub const PREVIEW_MAX_WORKLOADS: u16 = 32;

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
    /// Validates the stable Published Language without requiring consumers to
    /// import Developer Workflows' aggregate model.
    ///
    /// The owner performs the stronger aggregate reconstruction check before
    /// publication. This check protects foreign-context anti-corruption
    /// adapters from malformed or non-canonical integration data.
    pub fn validate(&self) -> Result<(), String> {
        if self.source_pull_request_change_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.source_environment_id.as_uuid().is_nil()
            || self.source_subscription_id.as_uuid().is_nil()
            || self.preview_policy_revision_id.as_uuid().is_nil()
            || self.preview_policy_revision_number == 0
            || self.preview_policy_revision_number > i64::MAX as u64
            || self.preview_id.as_uuid().is_nil()
            || self.preview_aggregate_version == 0
            || self.preview_aggregate_version > i64::MAX as u64
            || self.environment_id.as_uuid().is_nil()
            || self.environment_id == self.source_environment_id
            || self.owner_principal_id.as_uuid().is_nil()
            || self.installation_id == 0
            || self.installation_id > i64::MAX as u64
            || self.pull_request_id == 0
            || self.pull_request_id > i64::MAX as u64
            || self.pull_request_number == 0
            || self.pull_request_number > i64::MAX as u64
            || self.preview_policy_accepted_at
                != canonical_timestamp(self.preview_policy_accepted_at)
            || self.provider_created_at != canonical_timestamp(self.provider_created_at)
            || self.last_provider_updated_at != canonical_timestamp(self.last_provider_updated_at)
            || self.expires_at != canonical_timestamp(self.expires_at)
            || self
                .cleanup_requested_at
                .is_some_and(|value| value != canonical_timestamp(value))
            || self.provider_created_at > self.last_provider_updated_at
            || self.expires_at <= self.last_provider_updated_at
            || self.head_commit_sha.bytes().all(|byte| byte == b'0')
            || !matches!(
                self.last_change_kind.as_str(),
                "opened" | "synchronized" | "reopened" | "closed"
            )
            || !matches!(self.fork_policy.as_str(), "deny" | "isolated")
            || self.lifetime_seconds < PREVIEW_MIN_LIFETIME_SECONDS
            || self.lifetime_seconds > PREVIEW_MAX_LIFETIME_SECONDS
            || self.maximum_active_previews == 0
            || self.maximum_active_previews > PREVIEW_MAX_ACTIVE_PER_POLICY
            || self.maximum_workloads == 0
            || self.maximum_workloads > PREVIEW_MAX_WORKLOADS
            || self.cpu_millis == 0
            || self.cpu_millis > PREVIEW_MAX_CPU_MILLIS
            || !(PREVIEW_MIN_MEMORY_BYTES..=PREVIEW_MAX_MEMORY_BYTES).contains(&self.memory_bytes)
            || !self.memory_bytes.is_multiple_of(1024 * 1024)
            || !(PREVIEW_MIN_STORAGE_BYTES..=PREVIEW_MAX_STORAGE_BYTES)
                .contains(&self.ephemeral_storage_bytes)
            || !self.ephemeral_storage_bytes.is_multiple_of(1024 * 1024)
        {
            return Err(
                "Preview lifecycle Published Language identity or bounds are invalid".into(),
            );
        }

        let base_repository = validate_repository(
            &self.base_repository_provider,
            &self.base_repository_url,
            &self.base_repository_identity,
        )?;
        let head_repository = match (
            self.head_repository_provider.as_deref(),
            self.head_repository_url.as_deref(),
            self.head_repository_identity.as_deref(),
        ) {
            (None, None, None) => None,
            (Some(provider), Some(url), Some(identity)) => {
                Some(validate_repository(provider, url, identity)?)
            }
            _ => return Err("Preview lifecycle head repository binding is incomplete".into()),
        };
        validate_branch(&self.base_branch)?;
        validate_branch(&self.head_branch)?;
        GitCommitSha::parse(&self.head_commit_sha)?;

        let expected_environment_name = format!(
            "pr-{}-{}",
            self.pull_request_number,
            self.preview_id.as_uuid().simple()
        );
        let derived_is_fork = head_repository
            .as_ref()
            .is_none_or(|repository| repository != &base_repository);
        if self.environment_name != expected_environment_name
            || self.is_fork != derived_is_fork
            || self.protected_secrets_eligible
                != (self.is_active()
                    && self.allow_protected_secrets_for_trusted_sources
                    && !derived_is_fork)
        {
            return Err("Preview lifecycle Published Language derivation is invalid".into());
        }

        match (
            self.status.as_str(),
            self.cleanup_reason.as_deref(),
            self.cleanup_requested_at,
        ) {
            ("active", None, None)
                if self.last_change_kind != "closed"
                    && head_repository.is_some()
                    && !(self.fork_policy == "deny" && derived_is_fork) => {}
            ("cleanup_required", Some("pull_request_closed"), Some(requested_at))
                if self.last_change_kind == "closed"
                    && !self.last_merged
                    && requested_at == self.last_provider_updated_at => {}
            ("cleanup_required", Some("pull_request_merged"), Some(requested_at))
                if self.last_change_kind == "closed"
                    && self.last_merged
                    && requested_at == self.last_provider_updated_at => {}
            ("cleanup_required", Some("fork_denied"), Some(requested_at))
                if self.last_change_kind != "closed"
                    && self.fork_policy == "deny"
                    && derived_is_fork
                    && requested_at == self.last_provider_updated_at => {}
            ("cleanup_required", Some("expired"), Some(requested_at))
                if self.last_change_kind != "closed" && requested_at >= self.expires_at => {}
            _ => return Err("Preview lifecycle Published Language state is invalid".into()),
        }
        if self.last_merged && self.last_change_kind != "closed" {
            return Err("Preview lifecycle merge evidence is invalid".into());
        }
        Ok(())
    }

    pub fn is_active(&self) -> bool {
        self.status == "active"
    }
}

fn validate_repository(provider: &str, url: &str, identity: &str) -> Result<GitRepository, String> {
    let repository = GitRepository::parse(GitProvider::parse(provider)?, url)?;
    if repository.canonical_url() != url || repository.identity() != identity {
        return Err("Preview lifecycle repository binding is not canonical".into());
    }
    Ok(repository)
}

fn validate_branch(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 255
        || value == "@"
        || value.starts_with("refs/")
        || value.starts_with('/')
        || value.ends_with('/')
        || value.ends_with('.')
        || value.contains("..")
        || value.contains("//")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
        || value.split('/').any(|segment| {
            segment.is_empty()
                || segment.starts_with('.')
                || segment.ends_with('.')
                || segment.ends_with(".lock")
        })
    {
        return Err("Preview lifecycle branch is not a bounded canonical name".into());
    }
    Ok(())
}
