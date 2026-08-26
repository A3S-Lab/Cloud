use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GitCommitSha, OrganizationId, ProjectId,
    PullRequestPreviewId, Sha256Digest, SourcePullRequestChangeId, SourceRevisionId,
    SourceSubscriptionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_EVENT_KEY: &str =
    "source.pull-request-preview-revision.lifecycle-committed";
pub const PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewSourceRevisionLifecycleState {
    Active,
    CleanupRequired,
    SuppressedInactiveSubscription,
}

impl PreviewSourceRevisionLifecycleState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::CleanupRequired => "cleanup_required",
            Self::SuppressedInactiveSubscription => "suppressed_inactive_subscription",
        }
    }
}

/// Versioned Sources-owned lifecycle fact for one Preview's SourceRevision
/// binding. Artifacts can consume this fact with the same Preview version
/// fence without reading Sources or Developer Workflows storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewSourceRevisionLifecycleCommittedFact {
    source_pull_request_change_id: SourcePullRequestChangeId,
    organization_id: OrganizationId,
    project_id: ProjectId,
    source_environment_id: EnvironmentId,
    source_subscription_id: SourceSubscriptionId,
    preview_id: PullRequestPreviewId,
    preview_aggregate_version: u64,
    preview_environment_id: EnvironmentId,
    state: PreviewSourceRevisionLifecycleState,
    source_revision_id: Option<SourceRevisionId>,
    repository_identity: Option<String>,
    commit_sha: Option<String>,
    recipe_digest: Option<String>,
    source_revision_accepted_at: Option<DateTime<Utc>>,
}

impl PreviewSourceRevisionLifecycleCommittedFact {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::modules::sources) fn new(
        source_pull_request_change_id: SourcePullRequestChangeId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        preview_id: PullRequestPreviewId,
        preview_aggregate_version: u64,
        preview_environment_id: EnvironmentId,
        state: PreviewSourceRevisionLifecycleState,
        source_revision_id: Option<SourceRevisionId>,
        repository_identity: Option<String>,
        commit_sha: Option<String>,
        recipe_digest: Option<String>,
        source_revision_accepted_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            source_pull_request_change_id,
            organization_id,
            project_id,
            source_environment_id,
            source_subscription_id,
            preview_id,
            preview_aggregate_version,
            preview_environment_id,
            state,
            source_revision_id,
            repository_identity,
            commit_sha,
            recipe_digest,
            source_revision_accepted_at,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.source_pull_request_change_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.source_environment_id.as_uuid().is_nil()
            || self.source_subscription_id.as_uuid().is_nil()
            || self.preview_id.as_uuid().is_nil()
            || self.preview_aggregate_version == 0
            || self.preview_aggregate_version > i64::MAX as u64
            || self.preview_environment_id.as_uuid().is_nil()
            || self.preview_environment_id == self.source_environment_id
        {
            return Err("Preview Source revision lifecycle identity is invalid".into());
        }
        match (
            self.state,
            self.source_revision_id,
            self.repository_identity.as_deref(),
            self.commit_sha.as_deref(),
            self.recipe_digest.as_deref(),
            self.source_revision_accepted_at,
        ) {
            (
                PreviewSourceRevisionLifecycleState::Active,
                Some(revision_id),
                Some(repository),
                Some(commit),
                Some(recipe),
                Some(accepted_at),
            ) if !revision_id.as_uuid().is_nil()
                && !repository.trim().is_empty()
                && repository.len() <= 2_048
                && accepted_at == canonical_timestamp(accepted_at) =>
            {
                GitCommitSha::parse(commit)?;
                Sha256Digest::parse(recipe)?;
            }
            (
                PreviewSourceRevisionLifecycleState::CleanupRequired,
                None,
                None,
                None,
                None,
                None,
            )
            | (
                PreviewSourceRevisionLifecycleState::SuppressedInactiveSubscription,
                None,
                None,
                None,
                None,
                None,
            ) => {}
            _ => return Err("Preview Source revision lifecycle state is invalid".into()),
        }
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

    pub const fn source_environment_id(&self) -> EnvironmentId {
        self.source_environment_id
    }

    pub const fn source_subscription_id(&self) -> SourceSubscriptionId {
        self.source_subscription_id
    }

    pub const fn preview_id(&self) -> PullRequestPreviewId {
        self.preview_id
    }

    pub const fn preview_aggregate_version(&self) -> u64 {
        self.preview_aggregate_version
    }

    pub const fn preview_environment_id(&self) -> EnvironmentId {
        self.preview_environment_id
    }

    pub const fn state(&self) -> PreviewSourceRevisionLifecycleState {
        self.state
    }

    pub const fn source_revision_id(&self) -> Option<SourceRevisionId> {
        self.source_revision_id
    }

    pub fn repository_identity(&self) -> Option<&str> {
        self.repository_identity.as_deref()
    }

    pub fn commit_sha(&self) -> Option<&str> {
        self.commit_sha.as_deref()
    }

    pub fn recipe_digest(&self) -> Option<&str> {
        self.recipe_digest.as_deref()
    }

    /// Stable creation time of the ordinary Sources revision. Consumers must
    /// not substitute the enclosing Preview lifecycle time because multiple
    /// Preview versions may intentionally reuse the same revision identity.
    pub const fn source_revision_accepted_at(&self) -> Option<DateTime<Utc>> {
        self.source_revision_accepted_at
    }
}
