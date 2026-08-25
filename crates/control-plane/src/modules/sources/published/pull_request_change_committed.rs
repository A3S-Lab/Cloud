use super::{GitProvider, GitRepository};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GitCommitSha, OrganizationId, ProjectId,
    SourcePullRequestChangeId, SourceSubscriptionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const PULL_REQUEST_CHANGE_COMMITTED_EVENT_KEY: &str = "source.pull-request-change.committed";
pub const PULL_REQUEST_CHANGE_COMMITTED_SCHEMA_VERSION: u32 = 1;

/// Closed lifecycle vocabulary published by Sources after provider evidence is
/// authenticated and committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourcePullRequestChangeKind {
    Opened,
    Synchronized,
    Reopened,
    Closed,
}

impl SourcePullRequestChangeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Opened => "opened",
            Self::Synchronized => "synchronized",
            Self::Reopened => "reopened",
            Self::Closed => "closed",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Closed)
    }
}

/// Immutable Sources Published Language for one active Subscription's view of
/// an authenticated pull-request change.
///
/// Provider delivery IDs, signatures, raw payloads, and raw-payload digests
/// deliberately remain in the Sources inbox. Consumers receive only the
/// minimal semantic observation and exact Subscription binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PullRequestChangeCommittedFact {
    source_pull_request_change_id: SourcePullRequestChangeId,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    source_subscription_id: SourceSubscriptionId,
    installation_id: u64,
    base_repository: GitRepository,
    base_branch: String,
    head_repository: Option<GitRepository>,
    head_branch: String,
    head_commit_sha: String,
    pull_request_id: u64,
    pull_request_number: u64,
    kind: SourcePullRequestChangeKind,
    merged: bool,
    provider_created_at: DateTime<Utc>,
    provider_updated_at: DateTime<Utc>,
}

impl PullRequestChangeCommittedFact {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::modules::sources) fn new(
        source_pull_request_change_id: SourcePullRequestChangeId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        installation_id: u64,
        base_repository: GitRepository,
        base_branch: String,
        head_repository: Option<GitRepository>,
        head_branch: String,
        head_commit_sha: String,
        pull_request_id: u64,
        pull_request_number: u64,
        kind: SourcePullRequestChangeKind,
        merged: bool,
        provider_created_at: DateTime<Utc>,
        provider_updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            source_pull_request_change_id,
            organization_id,
            project_id,
            environment_id,
            source_subscription_id,
            installation_id,
            base_repository,
            base_branch,
            head_repository,
            head_branch,
            head_commit_sha,
            pull_request_id,
            pull_request_number,
            kind,
            merged,
            provider_created_at,
            provider_updated_at,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.source_pull_request_change_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.source_subscription_id.as_uuid().is_nil()
            || self.installation_id == 0
            || self.installation_id > i64::MAX as u64
            || self.pull_request_id == 0
            || self.pull_request_id > i64::MAX as u64
            || self.pull_request_number == 0
            || self.pull_request_number > i64::MAX as u64
            || self.base_repository.provider() != GitProvider::Github
            || self
                .head_repository
                .as_ref()
                .is_some_and(|repository| repository.provider() != GitProvider::Github)
            || self.head_commit_sha.bytes().all(|byte| byte == b'0')
            || self.provider_created_at != canonical_timestamp(self.provider_created_at)
            || self.provider_updated_at != canonical_timestamp(self.provider_updated_at)
            || self.provider_created_at > self.provider_updated_at
            || !self.kind.is_terminal() && self.merged
            || !self.kind.is_terminal() && self.head_repository.is_none()
        {
            return Err("committed pull-request change identity or state is invalid".into());
        }
        validate_repository(&self.base_repository)?;
        if let Some(repository) = &self.head_repository {
            validate_repository(repository)?;
        }
        validate_branch(&self.base_branch)?;
        validate_branch(&self.head_branch)?;
        GitCommitSha::parse(&self.head_commit_sha)?;
        Ok(())
    }

    pub const fn source_pull_request_change_id(&self) -> SourcePullRequestChangeId {
        self.source_pull_request_change_id
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn environment_id(&self) -> EnvironmentId {
        self.environment_id
    }

    pub const fn source_subscription_id(&self) -> SourceSubscriptionId {
        self.source_subscription_id
    }

    pub const fn installation_id(&self) -> u64 {
        self.installation_id
    }

    pub fn base_repository(&self) -> &GitRepository {
        &self.base_repository
    }

    pub fn base_branch(&self) -> &str {
        &self.base_branch
    }

    pub fn head_repository(&self) -> Option<&GitRepository> {
        self.head_repository.as_ref()
    }

    pub fn head_branch(&self) -> &str {
        &self.head_branch
    }

    pub fn head_commit_sha(&self) -> &str {
        &self.head_commit_sha
    }

    pub const fn pull_request_id(&self) -> u64 {
        self.pull_request_id
    }

    pub const fn pull_request_number(&self) -> u64 {
        self.pull_request_number
    }

    pub const fn kind(&self) -> SourcePullRequestChangeKind {
        self.kind
    }

    pub const fn merged(&self) -> bool {
        self.merged
    }

    pub const fn provider_created_at(&self) -> DateTime<Utc> {
        self.provider_created_at
    }

    pub const fn provider_updated_at(&self) -> DateTime<Utc> {
        self.provider_updated_at
    }
}

fn validate_repository(repository: &GitRepository) -> Result<(), String> {
    let canonical = GitRepository::parse(repository.provider(), repository.canonical_url())?;
    if &canonical != repository {
        return Err("committed pull-request repository is not canonical".into());
    }
    Ok(())
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
        return Err("committed pull-request branch is not a bounded canonical name".into());
    }
    Ok(())
}
