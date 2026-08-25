use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GitCommitSha, OrganizationId, PrincipalId, ProjectId,
    PullRequestPreviewId, SourceSubscriptionId,
};
use crate::modules::sources::published::{GitProvider, GitRepository};
use chrono::{DateTime, TimeDelta, Utc};
use std::cmp::Ordering;
use uuid::Uuid;

pub const MIN_PREVIEW_LIFETIME_SECONDS: u32 = 5 * 60;
pub const MAX_PREVIEW_LIFETIME_SECONDS: u32 = 30 * 24 * 60 * 60;
pub const MAX_ACTIVE_PREVIEWS_PER_POLICY: u16 = 256;

const MIN_PREVIEW_MEMORY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PREVIEW_MEMORY_BYTES: u64 = 512 * 1024 * 1024 * 1024;
const MIN_PREVIEW_STORAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PREVIEW_STORAGE_BYTES: u64 = 4 * 1024 * 1024 * 1024 * 1024;
const MAX_PREVIEW_CPU_MILLIS: u64 = 128_000;
const MAX_PREVIEW_WORKLOADS: u16 = 32;
const PREVIEW_NAMESPACE: Uuid = Uuid::from_bytes([
    0xac, 0xf1, 0x2e, 0xb7, 0xb2, 0x20, 0x4c, 0x19, 0x85, 0x62, 0x23, 0x4a, 0x7e, 0x6d, 0x31, 0x55,
]);
const PREVIEW_ENVIRONMENT_NAMESPACE: Uuid = Uuid::from_bytes([
    0x1a, 0xa9, 0xdd, 0xbe, 0xb8, 0xe9, 0x4f, 0x31, 0xa2, 0x89, 0x4a, 0x4d, 0xc8, 0x69, 0x1d, 0x17,
]);

/// Exact reference to the Sources-owned GitHub installation authority.
/// Developer Workflows owns neither installation lifecycle nor credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GithubInstallationRef(u64);

impl GithubInstallationRef {
    pub fn parse(value: u64) -> Result<Self, String> {
        if value == 0 || value > i64::MAX as u64 {
            return Err("preview GitHub installation reference is invalid".into());
        }
        Ok(Self(value))
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// The preview context needs only a branch, not Sources' complete Git
/// reference vocabulary (tags, commits, and provider resolution).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GitBranch(String);

impl GitBranch {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 255
            || value == "@"
            || value.starts_with("refs/")
            || value.starts_with('/')
            || value.ends_with('/')
            || value.ends_with('.')
            || value.contains("..")
            || value.contains("//")
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/')
            })
            || value.split('/').any(|segment| {
                segment.is_empty()
                    || segment.starts_with('.')
                    || segment.ends_with('.')
                    || segment.ends_with(".lock")
            })
        {
            return Err("preview Git branch is not a bounded canonical name".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PullRequestChangeKind {
    Opened,
    Synchronized,
    Reopened,
    Closed,
}

impl PullRequestChangeKind {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Closed)
    }
}

/// Developer Workflows-owned observation used by Preview reconciliation.
///
/// It deliberately excludes webhook signatures, delivery payloads, and other
/// Sources internals. An application adapter maps a committed Sources fact to
/// this minimal semantic input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestChange {
    pub installation_id: GithubInstallationRef,
    pub base_repository: GitRepository,
    pub base_branch: GitBranch,
    pub head_repository: Option<GitRepository>,
    pub head_branch: GitBranch,
    pub head_commit_sha: GitCommitSha,
    pub pull_request_id: u64,
    pub pull_request_number: u64,
    pub kind: PullRequestChangeKind,
    pub merged: bool,
    pub provider_created_at: DateTime<Utc>,
    pub provider_updated_at: DateTime<Utc>,
}

impl PullRequestChange {
    pub fn validate(&self) -> Result<(), String> {
        validate_pull_request_identity(self.pull_request_id, self.pull_request_number)?;
        if self.base_repository.provider() != GitProvider::Github
            || self
                .head_repository
                .as_ref()
                .is_some_and(|repository| repository.provider() != GitProvider::Github)
            || self
                .head_commit_sha
                .as_str()
                .bytes()
                .all(|byte| byte == b'0')
            || self.provider_created_at != canonical_timestamp(self.provider_created_at)
            || self.provider_updated_at != canonical_timestamp(self.provider_updated_at)
            || self.provider_created_at > self.provider_updated_at
            || !self.kind.is_terminal() && self.merged
            || !self.kind.is_terminal() && self.head_repository.is_none()
        {
            return Err("preview pull-request change identity or state is invalid".into());
        }
        let base = GitRepository::parse(
            self.base_repository.provider(),
            self.base_repository.canonical_url(),
        )?;
        if base != self.base_repository {
            return Err("preview pull-request base repository is not canonical".into());
        }
        if let Some(repository) = &self.head_repository {
            let head = GitRepository::parse(repository.provider(), repository.canonical_url())?;
            if &head != repository {
                return Err("preview pull-request head repository is not canonical".into());
            }
        }
        GitBranch::parse(self.base_branch.as_str())?;
        GitBranch::parse(self.head_branch.as_str())?;
        GitCommitSha::parse(self.head_commit_sha.as_str())?;
        Ok(())
    }

    pub fn is_fork(&self) -> bool {
        self.head_repository
            .as_ref()
            .is_none_or(|repository| repository != &self.base_repository)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewForkPolicy {
    Deny,
    Isolated,
}

impl PreviewForkPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deny => "deny",
            Self::Isolated => "isolated",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "deny" => Ok(Self::Deny),
            "isolated" => Ok(Self::Isolated),
            _ => Err("preview fork policy is unsupported".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewQuota {
    pub maximum_workloads: u16,
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub ephemeral_storage_bytes: u64,
}

impl PreviewQuota {
    pub fn validate(&self) -> Result<(), String> {
        if self.maximum_workloads == 0
            || self.maximum_workloads > MAX_PREVIEW_WORKLOADS
            || self.cpu_millis == 0
            || self.cpu_millis > MAX_PREVIEW_CPU_MILLIS
            || !(MIN_PREVIEW_MEMORY_BYTES..=MAX_PREVIEW_MEMORY_BYTES).contains(&self.memory_bytes)
            || !self.memory_bytes.is_multiple_of(1024 * 1024)
            || !(MIN_PREVIEW_STORAGE_BYTES..=MAX_PREVIEW_STORAGE_BYTES)
                .contains(&self.ephemeral_storage_bytes)
            || !self.ephemeral_storage_bytes.is_multiple_of(1024 * 1024)
        {
            return Err("preview quota is outside the closed P0.3 bounds".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestPreviewPolicy {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub source_subscription_id: SourceSubscriptionId,
    pub owner_principal_id: PrincipalId,
    pub installation_id: GithubInstallationRef,
    pub base_repository: GitRepository,
    pub base_branch: GitBranch,
    pub lifetime_seconds: u32,
    pub maximum_active_previews: u16,
    pub fork_policy: PreviewForkPolicy,
    pub allow_protected_secrets_for_trusted_sources: bool,
    pub quota: PreviewQuota,
}

impl PullRequestPreviewPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.source_subscription_id.as_uuid().is_nil()
            || self.owner_principal_id.as_uuid().is_nil()
            || self.base_repository.provider() != GitProvider::Github
            || !(MIN_PREVIEW_LIFETIME_SECONDS..=MAX_PREVIEW_LIFETIME_SECONDS)
                .contains(&self.lifetime_seconds)
            || self.maximum_active_previews == 0
            || self.maximum_active_previews > MAX_ACTIVE_PREVIEWS_PER_POLICY
        {
            return Err("pull-request preview policy identity or bounds are invalid".into());
        }
        let repository = GitRepository::parse(
            self.base_repository.provider(),
            self.base_repository.canonical_url(),
        )?;
        if repository != self.base_repository {
            return Err("preview policy repository is not canonical".into());
        }
        if GitBranch::parse(self.base_branch.as_str())? != self.base_branch {
            return Err("preview policy base branch is not canonical".into());
        }
        self.quota.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewCleanupReason {
    PullRequestClosed,
    PullRequestMerged,
    ForkDenied,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullRequestPreviewStatus {
    Active,
    CleanupRequired {
        reason: PreviewCleanupReason,
        requested_at: DateTime<Utc>,
    },
}

impl PullRequestPreviewStatus {
    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestPreview {
    pub policy: PullRequestPreviewPolicy,
    pub id: PullRequestPreviewId,
    pub environment_id: EnvironmentId,
    pub pull_request_id: u64,
    pub pull_request_number: u64,
    pub head_repository: Option<GitRepository>,
    pub head_branch: GitBranch,
    pub head_commit_sha: GitCommitSha,
    pub provider_created_at: DateTime<Utc>,
    pub last_provider_updated_at: DateTime<Utc>,
    pub last_change_kind: PullRequestChangeKind,
    pub last_merged: bool,
    pub expires_at: DateTime<Utc>,
    pub status: PullRequestPreviewStatus,
    pub aggregate_version: u64,
}

impl PullRequestPreview {
    pub fn preview_id_for(
        policy: &PullRequestPreviewPolicy,
        pull_request_id: u64,
        pull_request_number: u64,
    ) -> Result<PullRequestPreviewId, String> {
        policy.validate()?;
        validate_pull_request_identity(pull_request_id, pull_request_number)?;
        let mut identity = Vec::with_capacity(128 + policy.base_repository.identity().len());
        identity.extend_from_slice(policy.organization_id.as_uuid().as_bytes());
        identity.extend_from_slice(policy.project_id.as_uuid().as_bytes());
        identity.extend_from_slice(policy.source_subscription_id.as_uuid().as_bytes());
        push_identity_part(&mut identity, policy.base_repository.provider().as_str())?;
        push_identity_part(&mut identity, policy.base_repository.identity())?;
        identity.extend_from_slice(&pull_request_id.to_be_bytes());
        identity.extend_from_slice(&pull_request_number.to_be_bytes());
        Ok(PullRequestPreviewId::from_uuid(Uuid::new_v5(
            &PREVIEW_NAMESPACE,
            &identity,
        )))
    }

    pub fn environment_id_for(preview_id: PullRequestPreviewId) -> EnvironmentId {
        EnvironmentId::from_uuid(Uuid::new_v5(
            &PREVIEW_ENVIRONMENT_NAMESPACE,
            preview_id.as_uuid().as_bytes(),
        ))
    }

    pub fn environment_name(&self) -> String {
        let suffix = &self.id.as_uuid().simple().to_string()[..8];
        format!("pr-{}-{suffix}", self.pull_request_number)
    }

    pub fn is_fork(&self) -> bool {
        self.head_repository
            .as_ref()
            .is_none_or(|repository| repository != &self.policy.base_repository)
    }

    pub fn protected_secrets_eligible(&self) -> bool {
        self.status.is_active()
            && self.policy.allow_protected_secrets_for_trusted_sources
            && !self.is_fork()
    }

    pub fn validate(&self) -> Result<(), String> {
        self.policy.validate()?;
        validate_pull_request_identity(self.pull_request_id, self.pull_request_number)?;
        let expected_id =
            Self::preview_id_for(&self.policy, self.pull_request_id, self.pull_request_number)?;
        let expected_expiry =
            preview_expiry(self.last_provider_updated_at, self.policy.lifetime_seconds)?;
        if self.id != expected_id
            || self.environment_id != Self::environment_id_for(expected_id)
            || self.aggregate_version == 0
            || self.provider_created_at != canonical_timestamp(self.provider_created_at)
            || self.last_provider_updated_at != canonical_timestamp(self.last_provider_updated_at)
            || self.expires_at != canonical_timestamp(self.expires_at)
            || self.provider_created_at > self.last_provider_updated_at
            || self.expires_at != expected_expiry
            || self
                .head_commit_sha
                .as_str()
                .bytes()
                .all(|byte| byte == b'0')
            || !self.last_change_kind.is_terminal() && self.last_merged
            || self.status.is_active() && self.last_change_kind.is_terminal()
            || !self.last_change_kind.is_terminal() && self.head_repository.is_none()
            || self.status.is_active()
                && matches!(self.policy.fork_policy, PreviewForkPolicy::Deny)
                && self.is_fork()
        {
            return Err("pull-request preview identity or lifecycle state is invalid".into());
        }
        if let Some(repository) = &self.head_repository {
            let canonical =
                GitRepository::parse(repository.provider(), repository.canonical_url())?;
            if &canonical != repository {
                return Err("pull-request preview head repository is not canonical".into());
            }
        }
        GitBranch::parse(self.head_branch.as_str())?;
        GitCommitSha::parse(self.head_commit_sha.as_str())?;
        match &self.status {
            PullRequestPreviewStatus::Active => {}
            PullRequestPreviewStatus::CleanupRequired {
                reason,
                requested_at,
            } => {
                let canonical_requested_at = canonical_timestamp(*requested_at);
                if canonical_requested_at != *requested_at {
                    return Err("preview cleanup timestamp is not canonical".into());
                }
                match reason {
                    PreviewCleanupReason::PullRequestClosed
                        if self.last_change_kind.is_terminal()
                            && !self.last_merged
                            && canonical_requested_at == self.last_provider_updated_at => {}
                    PreviewCleanupReason::PullRequestMerged
                        if self.last_change_kind.is_terminal()
                            && self.last_merged
                            && canonical_requested_at == self.last_provider_updated_at => {}
                    PreviewCleanupReason::ForkDenied
                        if !self.last_change_kind.is_terminal()
                            && matches!(self.policy.fork_policy, PreviewForkPolicy::Deny)
                            && self.is_fork()
                            && canonical_requested_at == self.last_provider_updated_at => {}
                    PreviewCleanupReason::Expired
                        if !self.last_change_kind.is_terminal()
                            && canonical_requested_at >= self.expires_at => {}
                    _ => {
                        return Err(
                            "preview cleanup reason does not match its lifecycle evidence".into(),
                        )
                    }
                }
            }
        }
        Ok(())
    }

    pub fn expire(&self, now: DateTime<Utc>) -> Result<Option<Self>, String> {
        self.validate()?;
        let now = canonical_timestamp(now);
        if !self.status.is_active() || now < self.expires_at {
            return Ok(None);
        }
        let mut expired = self.clone();
        expired.aggregate_version = expired
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "preview aggregate version overflowed".to_owned())?;
        expired.status = PullRequestPreviewStatus::CleanupRequired {
            reason: PreviewCleanupReason::Expired,
            requested_at: now,
        };
        expired.validate()?;
        Ok(Some(expired))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewReconcileOutcome {
    Created,
    Updated,
    Reactivated,
    CleanupRequired,
    ForkDenied,
    IgnoredDuplicate,
    IgnoredStale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewReconciliation {
    pub preview: Option<PullRequestPreview>,
    pub outcome: PreviewReconcileOutcome,
}

pub fn reconcile_pull_request_preview(
    policy: &PullRequestPreviewPolicy,
    current: Option<&PullRequestPreview>,
    change: &PullRequestChange,
) -> Result<PreviewReconciliation, String> {
    policy.validate()?;
    change.validate()?;
    validate_change_binding(policy, change)?;
    let id = PullRequestPreview::preview_id_for(
        policy,
        change.pull_request_id,
        change.pull_request_number,
    )?;
    if let Some(current) = current {
        current.validate()?;
        if current.policy != *policy
            || current.id != id
            || current.pull_request_id != change.pull_request_id
            || current.pull_request_number != change.pull_request_number
            || current.provider_created_at != change.provider_created_at
        {
            return Err("pull-request change does not match the current Preview authority".into());
        }
    }
    let known_fork = change
        .head_repository
        .as_ref()
        .is_some_and(|repository| repository != &policy.base_repository);
    let denied_active_fork = known_fork
        && !change.kind.is_terminal()
        && matches!(policy.fork_policy, PreviewForkPolicy::Deny);
    if denied_active_fork && current.is_none() {
        return Ok(PreviewReconciliation {
            preview: None,
            outcome: PreviewReconcileOutcome::ForkDenied,
        });
    }
    if let Some(current) = current {
        match compare_change(current, change) {
            Ordering::Less => {
                return Ok(PreviewReconciliation {
                    preview: Some(current.clone()),
                    outcome: PreviewReconcileOutcome::IgnoredStale,
                })
            }
            Ordering::Equal => {
                return Ok(PreviewReconciliation {
                    preview: Some(current.clone()),
                    outcome: PreviewReconcileOutcome::IgnoredDuplicate,
                })
            }
            Ordering::Greater => {}
        }
    }
    let expires_at = preview_expiry(change.provider_updated_at, policy.lifetime_seconds)?;
    let status = if denied_active_fork {
        PullRequestPreviewStatus::CleanupRequired {
            reason: PreviewCleanupReason::ForkDenied,
            requested_at: change.provider_updated_at,
        }
    } else {
        status_for(change)
    };
    let outcome = match current {
        Some(_) if denied_active_fork => PreviewReconcileOutcome::ForkDenied,
        None if change.kind.is_terminal() => PreviewReconcileOutcome::CleanupRequired,
        None => PreviewReconcileOutcome::Created,
        Some(_) if change.kind.is_terminal() => PreviewReconcileOutcome::CleanupRequired,
        Some(previous) if !previous.status.is_active() => PreviewReconcileOutcome::Reactivated,
        Some(_) => PreviewReconcileOutcome::Updated,
    };
    let value = PullRequestPreview {
        policy: policy.clone(),
        id,
        environment_id: PullRequestPreview::environment_id_for(id),
        pull_request_id: change.pull_request_id,
        pull_request_number: change.pull_request_number,
        head_repository: change.head_repository.clone(),
        head_branch: change.head_branch.clone(),
        head_commit_sha: change.head_commit_sha.clone(),
        provider_created_at: change.provider_created_at,
        last_provider_updated_at: change.provider_updated_at,
        last_change_kind: change.kind,
        last_merged: change.merged,
        expires_at,
        status,
        aggregate_version: current
            .map(|preview| {
                preview
                    .aggregate_version
                    .checked_add(1)
                    .ok_or_else(|| "preview aggregate version overflowed".to_owned())
            })
            .transpose()?
            .unwrap_or(1),
    };
    value.validate()?;
    Ok(PreviewReconciliation {
        preview: Some(value),
        outcome,
    })
}

fn validate_change_binding(
    policy: &PullRequestPreviewPolicy,
    change: &PullRequestChange,
) -> Result<(), String> {
    if change.installation_id != policy.installation_id
        || change.base_repository != policy.base_repository
        || change.base_branch != policy.base_branch
    {
        return Err("pull-request change is outside the Preview policy binding".into());
    }
    Ok(())
}

fn validate_pull_request_identity(id: u64, number: u64) -> Result<(), String> {
    if id == 0 || id > i64::MAX as u64 || number == 0 || number > i64::MAX as u64 {
        return Err("pull-request identity must use positive signed 64-bit integers".into());
    }
    Ok(())
}

fn preview_expiry(
    provider_updated_at: DateTime<Utc>,
    lifetime_seconds: u32,
) -> Result<DateTime<Utc>, String> {
    canonical_timestamp(provider_updated_at)
        .checked_add_signed(TimeDelta::seconds(i64::from(lifetime_seconds)))
        .ok_or_else(|| "preview expiration timestamp overflowed".to_owned())
        .map(canonical_timestamp)
}

fn status_for(change: &PullRequestChange) -> PullRequestPreviewStatus {
    if change.kind.is_terminal() {
        PullRequestPreviewStatus::CleanupRequired {
            reason: if change.merged {
                PreviewCleanupReason::PullRequestMerged
            } else {
                PreviewCleanupReason::PullRequestClosed
            },
            requested_at: change.provider_updated_at,
        }
    } else {
        PullRequestPreviewStatus::Active
    }
}

fn compare_change(current: &PullRequestPreview, change: &PullRequestChange) -> Ordering {
    change_order_key(
        change.provider_updated_at,
        change.kind,
        change.merged,
        change.head_repository.as_ref(),
        &change.head_branch,
        &change.head_commit_sha,
    )
    .cmp(&change_order_key(
        current.last_provider_updated_at,
        current.last_change_kind,
        current.last_merged,
        current.head_repository.as_ref(),
        &current.head_branch,
        &current.head_commit_sha,
    ))
}

fn change_order_key<'a>(
    updated_at: DateTime<Utc>,
    kind: PullRequestChangeKind,
    merged: bool,
    head_repository: Option<&'a GitRepository>,
    head_branch: &'a GitBranch,
    head_commit_sha: &'a GitCommitSha,
) -> (DateTime<Utc>, u8, bool, &'a str, &'a str, &'a str) {
    let rank = match kind {
        PullRequestChangeKind::Opened => 0,
        PullRequestChangeKind::Synchronized => 1,
        PullRequestChangeKind::Reopened => 2,
        PullRequestChangeKind::Closed => 3,
    };
    (
        updated_at,
        rank,
        merged,
        head_repository.map_or("", GitRepository::identity),
        head_branch.as_str(),
        head_commit_sha.as_str(),
    )
}

fn push_identity_part(identity: &mut Vec<u8>, value: &str) -> Result<(), String> {
    let length =
        u32::try_from(value.len()).map_err(|_| "preview identity field exceeds u32".to_owned())?;
    identity.extend_from_slice(&length.to_be_bytes());
    identity.extend_from_slice(value.as_bytes());
    Ok(())
}
