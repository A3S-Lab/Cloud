use super::PullRequestPreview;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, IdempotentWrite, OrganizationId, ProjectId,
    PullRequestPreviewId, PullRequestPreviewPolicyRevisionId, RepositoryError, Sha256Digest,
    SourcePullRequestChangeId, SourceSubscriptionId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestPreviewProjectionOutcome {
    NoApplicablePolicy,
    Created,
    Updated,
    Reactivated,
    CleanupRequired,
    ForkDenied,
    IgnoredDuplicate,
    IgnoredStale,
}

impl PullRequestPreviewProjectionOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoApplicablePolicy => "no_applicable_policy",
            Self::Created => "created",
            Self::Updated => "updated",
            Self::Reactivated => "reactivated",
            Self::CleanupRequired => "cleanup_required",
            Self::ForkDenied => "fork_denied",
            Self::IgnoredDuplicate => "ignored_duplicate",
            Self::IgnoredStale => "ignored_stale",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "no_applicable_policy" => Ok(Self::NoApplicablePolicy),
            "created" => Ok(Self::Created),
            "updated" => Ok(Self::Updated),
            "reactivated" => Ok(Self::Reactivated),
            "cleanup_required" => Ok(Self::CleanupRequired),
            "fork_denied" => Ok(Self::ForkDenied),
            "ignored_duplicate" => Ok(Self::IgnoredDuplicate),
            "ignored_stale" => Ok(Self::IgnoredStale),
            _ => Err("pull-request Preview projection outcome is unsupported".into()),
        }
    }
}

/// Consumer-owned immutable evidence that one Sources fact reached a terminal
/// projection decision.
///
/// This is not another transport Inbox, queue, or retry rail. The existing
/// Outbox Relay owns delivery and retry; this receipt only makes its local
/// materialization replay-safe and detects an opaque fact ID changing content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestPreviewProjectionReceipt {
    pub source_pull_request_change_id: SourcePullRequestChangeId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub source_environment_id: EnvironmentId,
    pub source_subscription_id: SourceSubscriptionId,
    pub pull_request_id: u64,
    pub pull_request_number: u64,
    pub fact_digest: Sha256Digest,
    pub fact_occurred_at: DateTime<Utc>,
    pub policy_revision_id: Option<PullRequestPreviewPolicyRevisionId>,
    pub preview_id: Option<PullRequestPreviewId>,
    pub preview_aggregate_version: Option<u64>,
    pub outcome: PullRequestPreviewProjectionOutcome,
}

impl PullRequestPreviewProjectionReceipt {
    pub fn restore(value: Self) -> Result<Self, String> {
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.source_pull_request_change_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.source_environment_id.as_uuid().is_nil()
            || self.source_subscription_id.as_uuid().is_nil()
            || self.pull_request_id == 0
            || self.pull_request_id > i64::MAX as u64
            || self.pull_request_number == 0
            || self.pull_request_number > i64::MAX as u64
            || self.fact_digest != Sha256Digest::parse(self.fact_digest.as_str())?
            || self.fact_occurred_at != canonical_timestamp(self.fact_occurred_at)
            || self
                .policy_revision_id
                .is_some_and(|id| id.as_uuid().is_nil())
            || self.preview_id.is_some_and(|id| id.as_uuid().is_nil())
            || self.preview_aggregate_version == Some(0)
            || self.preview_id.is_some() != self.preview_aggregate_version.is_some()
        {
            return Err("pull-request Preview projection receipt is invalid".into());
        }

        let shape_is_valid = match self.outcome {
            PullRequestPreviewProjectionOutcome::NoApplicablePolicy => {
                self.policy_revision_id.is_none() && self.preview_id.is_none()
            }
            PullRequestPreviewProjectionOutcome::ForkDenied => self.policy_revision_id.is_some(),
            PullRequestPreviewProjectionOutcome::Created
            | PullRequestPreviewProjectionOutcome::Updated
            | PullRequestPreviewProjectionOutcome::Reactivated
            | PullRequestPreviewProjectionOutcome::CleanupRequired
            | PullRequestPreviewProjectionOutcome::IgnoredDuplicate
            | PullRequestPreviewProjectionOutcome::IgnoredStale => {
                self.policy_revision_id.is_some() && self.preview_id.is_some()
            }
        };
        if !shape_is_valid {
            return Err("pull-request Preview projection outcome evidence is inconsistent".into());
        }
        Ok(())
    }

    pub fn matches_fact(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        pull_request_id: u64,
        pull_request_number: u64,
        fact_digest: &Sha256Digest,
        fact_occurred_at: DateTime<Utc>,
    ) -> bool {
        self.organization_id == organization_id
            && self.project_id == project_id
            && self.source_environment_id == source_environment_id
            && self.source_subscription_id == source_subscription_id
            && self.pull_request_id == pull_request_id
            && self.pull_request_number == pull_request_number
            && &self.fact_digest == fact_digest
            && self.fact_occurred_at == canonical_timestamp(fact_occurred_at)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PullRequestPreviewVersion {
    pub id: PullRequestPreviewId,
    pub aggregate_version: u64,
}

impl PullRequestPreviewVersion {
    pub fn validate(self) -> Result<(), String> {
        if self.id.as_uuid().is_nil() || self.aggregate_version == 0 {
            return Err("observed pull-request Preview version is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct CommitPullRequestPreviewProjection {
    pub receipt: PullRequestPreviewProjectionReceipt,
    /// Exact state observed by the Application service. `None` means absence
    /// was observed, rather than "do not compare".
    pub expected_preview: Option<PullRequestPreviewVersion>,
    /// Present only when this fact advances the Preview aggregate.
    pub preview: Option<PullRequestPreview>,
}

impl CommitPullRequestPreviewProjection {
    pub fn validate(&self) -> Result<(), String> {
        self.receipt.validate()?;
        if let Some(expected) = self.expected_preview {
            expected.validate()?;
        }
        let receipt_version = self
            .receipt
            .preview_id
            .zip(self.receipt.preview_aggregate_version)
            .map(|(id, aggregate_version)| PullRequestPreviewVersion {
                id,
                aggregate_version,
            });

        if let Some(preview) = &self.preview {
            preview.validate()?;
            let authority = &preview.policy_authority;
            let expected_before = preview
                .aggregate_version
                .checked_sub(1)
                .filter(|version| *version > 0)
                .map(|aggregate_version| PullRequestPreviewVersion {
                    id: preview.id,
                    aggregate_version,
                });
            if self.receipt.organization_id != authority.policy.organization_id
                || self.receipt.project_id != authority.policy.project_id
                || self.receipt.source_environment_id != authority.source_environment_id
                || self.receipt.source_subscription_id != authority.policy.source_subscription_id
                || self.receipt.pull_request_id != preview.pull_request_id
                || self.receipt.pull_request_number != preview.pull_request_number
                || self.receipt.policy_revision_id != Some(authority.revision_id)
                || receipt_version
                    != Some(PullRequestPreviewVersion {
                        id: preview.id,
                        aggregate_version: preview.aggregate_version,
                    })
                || self.expected_preview != expected_before
                || matches!(
                    self.receipt.outcome,
                    PullRequestPreviewProjectionOutcome::NoApplicablePolicy
                        | PullRequestPreviewProjectionOutcome::IgnoredDuplicate
                        | PullRequestPreviewProjectionOutcome::IgnoredStale
                )
            {
                return Err("Preview mutation and projection receipt are inconsistent".into());
            }
        } else if receipt_version != self.expected_preview {
            return Err("Preview observation and projection receipt are inconsistent".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait IPullRequestPreviewProjectionRepository: Send + Sync {
    async fn find_receipt(
        &self,
        organization_id: OrganizationId,
        source_pull_request_change_id: SourcePullRequestChangeId,
    ) -> Result<Option<PullRequestPreviewProjectionReceipt>, RepositoryError>;

    async fn find_preview(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        pull_request_id: u64,
    ) -> Result<Option<PullRequestPreview>, RepositoryError>;

    async fn commit_projection(
        &self,
        write: CommitPullRequestPreviewProjection,
    ) -> Result<IdempotentWrite<PullRequestPreviewProjectionReceipt>, RepositoryError>;
}
