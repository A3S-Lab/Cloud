use super::build_candidate::{
    BuildCandidate, BuildCandidateEvidence, IBuildCandidateProjectionPort,
};
use crate::modules::artifacts::domain::BuildSubject;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, BuildRunId, EnvironmentId, GitCommitSha, IdempotentWrite, OrganizationId,
    ProjectId, PullRequestPreviewId, RepositoryError, Sha256Digest, SourcePullRequestChangeId,
    SourceRevisionId, SourceSubscriptionId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewBuildLifecycleState {
    Active,
    CleanupRequired,
    SuppressedInactiveSubscription,
}

impl PreviewBuildLifecycleState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::CleanupRequired => "cleanup_required",
            Self::SuppressedInactiveSubscription => "suppressed_inactive_subscription",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "active" => Ok(Self::Active),
            "cleanup_required" => Ok(Self::CleanupRequired),
            "suppressed_inactive_subscription" => Ok(Self::SuppressedInactiveSubscription),
            _ => Err(format!(
                "unsupported Preview build lifecycle state {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewBuildSourceRevision {
    pub source_revision_id: SourceRevisionId,
    pub repository_identity: String,
    pub commit_sha: GitCommitSha,
    pub recipe_digest: Sha256Digest,
    pub accepted_at: DateTime<Utc>,
}

impl PreviewBuildSourceRevision {
    pub fn validate(&self, fact_occurred_at: DateTime<Utc>) -> Result<(), String> {
        if self.source_revision_id.as_uuid().is_nil()
            || self.repository_identity.trim().is_empty()
            || self.repository_identity.len() > 2_048
            || self.accepted_at != canonical_timestamp(self.accepted_at)
            || self.accepted_at > fact_occurred_at
        {
            return Err("Preview build SourceRevision evidence is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPreviewBuildLifecycle {
    pub lifecycle_event_id: Uuid,
    pub correlation_id: Uuid,
    pub lifecycle_causation_id: Uuid,
    pub source_pull_request_change_id: SourcePullRequestChangeId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub source_environment_id: EnvironmentId,
    pub source_subscription_id: SourceSubscriptionId,
    pub preview_id: PullRequestPreviewId,
    pub preview_aggregate_version: u64,
    pub preview_environment_id: EnvironmentId,
    pub state: PreviewBuildLifecycleState,
    pub source_revision: Option<PreviewBuildSourceRevision>,
    pub fact_occurred_at: DateTime<Utc>,
}

impl ProjectPreviewBuildLifecycle {
    pub fn validate(&self) -> Result<(), String> {
        if self.lifecycle_event_id.is_nil()
            || self.correlation_id.is_nil()
            || self.lifecycle_causation_id.is_nil()
            || self.source_pull_request_change_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.source_environment_id.as_uuid().is_nil()
            || self.source_subscription_id.as_uuid().is_nil()
            || self.preview_id.as_uuid().is_nil()
            || self.preview_aggregate_version == 0
            || self.preview_aggregate_version > i64::MAX as u64
            || self.preview_environment_id.as_uuid().is_nil()
            || self.preview_environment_id == self.source_environment_id
            || self.fact_occurred_at != canonical_timestamp(self.fact_occurred_at)
        {
            return Err("Preview build lifecycle identity is invalid".into());
        }
        match (self.state, &self.source_revision) {
            (PreviewBuildLifecycleState::Active, Some(revision)) => {
                revision.validate(self.fact_occurred_at)
            }
            (
                PreviewBuildLifecycleState::CleanupRequired
                | PreviewBuildLifecycleState::SuppressedInactiveSubscription,
                None,
            ) => Ok(()),
            _ => Err("Preview build lifecycle state is invalid".into()),
        }
    }

    pub fn candidate(&self) -> Result<Option<BuildCandidate>, String> {
        self.validate()?;
        self.source_revision
            .as_ref()
            .map(|revision| {
                BuildCandidate::for_preview_source_revision(
                    self.organization_id,
                    BuildSubject::external_source_revision(
                        self.project_id,
                        self.preview_environment_id,
                        revision.source_revision_id,
                    ),
                    self.preview_id,
                    BuildCandidateEvidence::external_source_revision(
                        revision.repository_identity.clone(),
                        revision.commit_sha.clone(),
                        revision.recipe_digest.clone(),
                    )?,
                    revision.accepted_at,
                )
            })
            .transpose()
    }

    pub(in crate::modules::artifacts) fn decide(
        &self,
        latest: Option<&PreviewBuildLifecycleProjectionReceipt>,
    ) -> Result<PreviewBuildLifecycleProjectionDecision, String> {
        self.validate()?;
        if let Some(latest) = latest {
            latest.validate()?;
            if !latest.has_same_scope_as(self) {
                return Err("Preview lifecycle changed its Artifacts projection scope".into());
            }
            if latest.preview_aggregate_version >= self.preview_aggregate_version {
                return Ok(PreviewBuildLifecycleProjectionDecision {
                    outcome: PreviewBuildLifecycleProjectionOutcome::IgnoredStale,
                    candidate: None,
                    retired_source_revision_id: None,
                });
            }
        }

        let current_source_revision_id = self
            .source_revision
            .as_ref()
            .map(|revision| revision.source_revision_id);
        let retired_source_revision_id = latest
            .filter(|receipt| receipt.outcome == PreviewBuildLifecycleProjectionOutcome::Applied)
            .and_then(PreviewBuildLifecycleProjectionReceipt::source_revision_id)
            .filter(|previous| Some(*previous) != current_source_revision_id);
        Ok(PreviewBuildLifecycleProjectionDecision {
            outcome: PreviewBuildLifecycleProjectionOutcome::Applied,
            candidate: self.candidate()?,
            retired_source_revision_id,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewBuildLifecycleProjectionOutcome {
    Applied,
    IgnoredStale,
}

impl PreviewBuildLifecycleProjectionOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::IgnoredStale => "ignored_stale",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "applied" => Ok(Self::Applied),
            "ignored_stale" => Ok(Self::IgnoredStale),
            _ => Err(format!(
                "unsupported Preview build projection outcome {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewBuildRetirement {
    NotRequired,
    PendingSuppressed {
        source_revision_id: SourceRevisionId,
    },
    CancellationRequested {
        source_revision_id: SourceRevisionId,
        build_run_id: BuildRunId,
    },
    TerminalObserved {
        source_revision_id: SourceRevisionId,
        build_run_id: BuildRunId,
    },
}

impl PreviewBuildRetirement {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotRequired => "not_required",
            Self::PendingSuppressed { .. } => "pending_suppressed",
            Self::CancellationRequested { .. } => "cancellation_requested",
            Self::TerminalObserved { .. } => "terminal_observed",
        }
    }

    pub const fn source_revision_id(self) -> Option<SourceRevisionId> {
        match self {
            Self::NotRequired => None,
            Self::PendingSuppressed { source_revision_id }
            | Self::CancellationRequested {
                source_revision_id, ..
            }
            | Self::TerminalObserved {
                source_revision_id, ..
            } => Some(source_revision_id),
        }
    }

    pub const fn build_run_id(self) -> Option<BuildRunId> {
        match self {
            Self::CancellationRequested { build_run_id, .. }
            | Self::TerminalObserved { build_run_id, .. } => Some(build_run_id),
            Self::NotRequired | Self::PendingSuppressed { .. } => None,
        }
    }

    pub fn restore(
        outcome: &str,
        source_revision_id: Option<SourceRevisionId>,
        build_run_id: Option<BuildRunId>,
    ) -> Result<Self, String> {
        match (outcome, source_revision_id, build_run_id) {
            ("not_required", None, None) => Ok(Self::NotRequired),
            ("pending_suppressed", Some(source_revision_id), None) => {
                Ok(Self::PendingSuppressed { source_revision_id })
            }
            ("cancellation_requested", Some(source_revision_id), Some(build_run_id)) => {
                Ok(Self::CancellationRequested {
                    source_revision_id,
                    build_run_id,
                })
            }
            ("terminal_observed", Some(source_revision_id), Some(build_run_id)) => {
                Ok(Self::TerminalObserved {
                    source_revision_id,
                    build_run_id,
                })
            }
            _ => Err("stored Preview build retirement shape is invalid".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewBuildLifecycleProjectionReceipt {
    pub lifecycle_event_id: Uuid,
    pub correlation_id: Uuid,
    pub lifecycle_causation_id: Uuid,
    pub source_pull_request_change_id: SourcePullRequestChangeId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub source_environment_id: EnvironmentId,
    pub source_subscription_id: SourceSubscriptionId,
    pub preview_id: PullRequestPreviewId,
    pub preview_aggregate_version: u64,
    pub preview_environment_id: EnvironmentId,
    pub state: PreviewBuildLifecycleState,
    pub source_revision: Option<PreviewBuildSourceRevision>,
    pub fact_occurred_at: DateTime<Utc>,
    pub outcome: PreviewBuildLifecycleProjectionOutcome,
    pub retirement: PreviewBuildRetirement,
}

impl PreviewBuildLifecycleProjectionReceipt {
    pub fn from_input(
        input: &ProjectPreviewBuildLifecycle,
        outcome: PreviewBuildLifecycleProjectionOutcome,
        retirement: PreviewBuildRetirement,
    ) -> Result<Self, String> {
        let receipt = Self {
            lifecycle_event_id: input.lifecycle_event_id,
            correlation_id: input.correlation_id,
            lifecycle_causation_id: input.lifecycle_causation_id,
            source_pull_request_change_id: input.source_pull_request_change_id,
            organization_id: input.organization_id,
            project_id: input.project_id,
            source_environment_id: input.source_environment_id,
            source_subscription_id: input.source_subscription_id,
            preview_id: input.preview_id,
            preview_aggregate_version: input.preview_aggregate_version,
            preview_environment_id: input.preview_environment_id,
            state: input.state,
            source_revision: input.source_revision.clone(),
            fact_occurred_at: input.fact_occurred_at,
            outcome,
            retirement,
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn restore(receipt: Self) -> Result<Self, String> {
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        ProjectPreviewBuildLifecycle {
            lifecycle_event_id: self.lifecycle_event_id,
            correlation_id: self.correlation_id,
            lifecycle_causation_id: self.lifecycle_causation_id,
            source_pull_request_change_id: self.source_pull_request_change_id,
            organization_id: self.organization_id,
            project_id: self.project_id,
            source_environment_id: self.source_environment_id,
            source_subscription_id: self.source_subscription_id,
            preview_id: self.preview_id,
            preview_aggregate_version: self.preview_aggregate_version,
            preview_environment_id: self.preview_environment_id,
            state: self.state,
            source_revision: self.source_revision.clone(),
            fact_occurred_at: self.fact_occurred_at,
        }
        .validate()?;
        if self.outcome == PreviewBuildLifecycleProjectionOutcome::IgnoredStale
            && self.retirement != PreviewBuildRetirement::NotRequired
        {
            return Err("stale Preview build projection performed retirement".into());
        }
        if self
            .retirement
            .source_revision_id()
            .is_some_and(|id| id.as_uuid().is_nil() || Some(id) == self.source_revision_id())
            || self
                .retirement
                .build_run_id()
                .is_some_and(|id| id.as_uuid().is_nil())
        {
            return Err("Preview build retirement binding is invalid".into());
        }
        Ok(())
    }

    pub fn matches_input(&self, input: &ProjectPreviewBuildLifecycle) -> bool {
        self.lifecycle_event_id == input.lifecycle_event_id
            && self.correlation_id == input.correlation_id
            && self.lifecycle_causation_id == input.lifecycle_causation_id
            && self.source_pull_request_change_id == input.source_pull_request_change_id
            && self.organization_id == input.organization_id
            && self.project_id == input.project_id
            && self.source_environment_id == input.source_environment_id
            && self.source_subscription_id == input.source_subscription_id
            && self.preview_id == input.preview_id
            && self.preview_aggregate_version == input.preview_aggregate_version
            && self.preview_environment_id == input.preview_environment_id
            && self.state == input.state
            && self.source_revision == input.source_revision
            && self.fact_occurred_at == input.fact_occurred_at
    }

    pub fn has_same_scope_as(&self, input: &ProjectPreviewBuildLifecycle) -> bool {
        self.organization_id == input.organization_id
            && self.project_id == input.project_id
            && self.source_environment_id == input.source_environment_id
            && self.source_subscription_id == input.source_subscription_id
            && self.preview_id == input.preview_id
            && self.preview_environment_id == input.preview_environment_id
    }

    pub fn source_revision_id(&self) -> Option<SourceRevisionId> {
        self.source_revision
            .as_ref()
            .map(|revision| revision.source_revision_id)
    }
}

pub(in crate::modules::artifacts) struct PreviewBuildLifecycleProjectionDecision {
    pub outcome: PreviewBuildLifecycleProjectionOutcome,
    pub candidate: Option<BuildCandidate>,
    pub retired_source_revision_id: Option<SourceRevisionId>,
}

#[async_trait]
pub trait IPreviewBuildLifecycleProjectionPort: Send + Sync {
    async fn project_preview_build_lifecycle(
        &self,
        input: ProjectPreviewBuildLifecycle,
    ) -> Result<IdempotentWrite<PreviewBuildLifecycleProjectionReceipt>, RepositoryError>;
}

/// One composition authority for every owner fact that can admit or retire an
/// Artifacts BuildCandidate. It combines ports without merging their semantics.
pub trait IArtifactBuildProjectionPort:
    IBuildCandidateProjectionPort + IPreviewBuildLifecycleProjectionPort
{
}

impl<T> IArtifactBuildProjectionPort for T where
    T: IBuildCandidateProjectionPort + IPreviewBuildLifecycleProjectionPort
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    #[test]
    fn newer_lifecycle_retires_only_a_different_active_revision() {
        let first = input(
            1,
            PreviewBuildLifecycleState::Active,
            Some(SourceRevisionId::new()),
        );
        let first_receipt = PreviewBuildLifecycleProjectionReceipt::from_input(
            &first,
            PreviewBuildLifecycleProjectionOutcome::Applied,
            PreviewBuildRetirement::NotRequired,
        )
        .expect("first receipt");
        let same = ProjectPreviewBuildLifecycle {
            lifecycle_event_id: Uuid::now_v7(),
            source_pull_request_change_id: SourcePullRequestChangeId::new(),
            preview_aggregate_version: 2,
            fact_occurred_at: first.fact_occurred_at + Duration::seconds(1),
            ..first.clone()
        };
        let same_decision = same.decide(Some(&first_receipt)).expect("same revision");
        assert!(same_decision.retired_source_revision_id.is_none());

        let next_revision_id = SourceRevisionId::new();
        let next = input(
            2,
            PreviewBuildLifecycleState::Active,
            Some(next_revision_id),
        );
        let next = ProjectPreviewBuildLifecycle {
            organization_id: first.organization_id,
            project_id: first.project_id,
            source_environment_id: first.source_environment_id,
            source_subscription_id: first.source_subscription_id,
            preview_id: first.preview_id,
            preview_environment_id: first.preview_environment_id,
            ..next
        };
        let decision = next.decide(Some(&first_receipt)).expect("next revision");
        assert_eq!(
            decision.retired_source_revision_id,
            first
                .source_revision
                .map(|revision| revision.source_revision_id)
        );
        assert_eq!(
            decision.candidate.expect("new candidate").preview_id(),
            Some(first.preview_id)
        );
    }

    #[test]
    fn inactive_and_stale_lifecycle_never_admit_a_candidate() {
        let active = input(
            2,
            PreviewBuildLifecycleState::Active,
            Some(SourceRevisionId::new()),
        );
        let active_receipt = PreviewBuildLifecycleProjectionReceipt::from_input(
            &active,
            PreviewBuildLifecycleProjectionOutcome::Applied,
            PreviewBuildRetirement::NotRequired,
        )
        .expect("active receipt");
        let inactive = ProjectPreviewBuildLifecycle {
            lifecycle_event_id: Uuid::now_v7(),
            source_pull_request_change_id: SourcePullRequestChangeId::new(),
            preview_aggregate_version: 3,
            state: PreviewBuildLifecycleState::CleanupRequired,
            source_revision: None,
            fact_occurred_at: active.fact_occurred_at + Duration::seconds(1),
            ..active.clone()
        };
        let inactive_decision = inactive
            .decide(Some(&active_receipt))
            .expect("inactive decision");
        assert!(inactive_decision.candidate.is_none());
        assert_eq!(
            inactive_decision.retired_source_revision_id,
            active
                .source_revision
                .as_ref()
                .map(|revision| revision.source_revision_id)
        );

        let stale = ProjectPreviewBuildLifecycle {
            lifecycle_event_id: Uuid::now_v7(),
            source_pull_request_change_id: SourcePullRequestChangeId::new(),
            preview_aggregate_version: 1,
            ..active.clone()
        };
        let stale_decision = stale.decide(Some(&active_receipt)).expect("stale decision");
        assert_eq!(
            stale_decision.outcome,
            PreviewBuildLifecycleProjectionOutcome::IgnoredStale
        );
        assert!(stale_decision.candidate.is_none());
        assert!(stale_decision.retired_source_revision_id.is_none());
    }

    fn input(
        version: u64,
        state: PreviewBuildLifecycleState,
        source_revision_id: Option<SourceRevisionId>,
    ) -> ProjectPreviewBuildLifecycle {
        let fact_occurred_at = Utc
            .with_ymd_and_hms(2026, 8, 26, 0, 0, version as u32)
            .single()
            .expect("time");
        ProjectPreviewBuildLifecycle {
            lifecycle_event_id: Uuid::now_v7(),
            correlation_id: Uuid::now_v7(),
            lifecycle_causation_id: Uuid::now_v7(),
            source_pull_request_change_id: SourcePullRequestChangeId::new(),
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            source_environment_id: EnvironmentId::new(),
            source_subscription_id: SourceSubscriptionId::new(),
            preview_id: PullRequestPreviewId::new(),
            preview_aggregate_version: version,
            preview_environment_id: EnvironmentId::new(),
            state,
            source_revision: source_revision_id.map(|source_revision_id| {
                PreviewBuildSourceRevision {
                    source_revision_id,
                    repository_identity: "github:github.com/a3s-lab/cloud".into(),
                    commit_sha: GitCommitSha::parse("a".repeat(40)).expect("commit"),
                    recipe_digest: Sha256Digest::parse(format!("sha256:{}", "b".repeat(64)))
                        .expect("recipe digest"),
                    accepted_at: fact_occurred_at,
                }
            }),
            fact_occurred_at,
        }
    }
}
