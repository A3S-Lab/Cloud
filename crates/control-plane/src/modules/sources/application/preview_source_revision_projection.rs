use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GitCommitSha, IdempotentWrite, OrganizationId, ProjectId,
    PullRequestPreviewId, RepositoryError, Sha256Digest, SourcePullRequestChangeId,
    SourceRevisionId, SourceSubscriptionId,
};
use crate::modules::sources::domain::{
    ExternalSourceRevision, GitProvider, GitReference, GitRepository, GithubInstallationId,
    GithubRepositorySubscription, NewExternalSourceRevision,
};
use crate::modules::sources::published::{
    PreviewSourceRevisionLifecycleCommittedFact, PreviewSourceRevisionLifecycleState,
    PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_EVENT_KEY,
    PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_SCHEMA_VERSION,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewSourceRevisionDesiredState {
    Active,
    CleanupRequired,
}

impl PreviewSourceRevisionDesiredState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::CleanupRequired => "cleanup_required",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewSourceRevisionProjectionOutcome {
    Projected,
    CleanupRequired,
    SuppressedInactiveSubscription,
    IgnoredStale,
}

impl PreviewSourceRevisionProjectionOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Projected => "projected",
            Self::CleanupRequired => "cleanup_required",
            Self::SuppressedInactiveSubscription => "suppressed_inactive_subscription",
            Self::IgnoredStale => "ignored_stale",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "projected" => Ok(Self::Projected),
            "cleanup_required" => Ok(Self::CleanupRequired),
            "suppressed_inactive_subscription" => Ok(Self::SuppressedInactiveSubscription),
            "ignored_stale" => Ok(Self::IgnoredStale),
            _ => Err("Preview Source revision projection outcome is invalid".into()),
        }
    }
}

/// Sources-local command translated from Developer Workflows Published
/// Language. It contains no Preview aggregate or foreign repository handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPreviewSourceRevision {
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
    pub installation_id: GithubInstallationId,
    pub base_repository: GitRepository,
    pub base_branch: GitReference,
    pub head_repository: Option<GitRepository>,
    pub head_branch: GitReference,
    pub head_commit_sha: GitCommitSha,
    pub pull_request_id: u64,
    pub pull_request_number: u64,
    pub desired_state: PreviewSourceRevisionDesiredState,
    pub fact_digest: Sha256Digest,
    pub fact_occurred_at: DateTime<Utc>,
}

impl ProjectPreviewSourceRevision {
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
            || self.pull_request_id == 0
            || self.pull_request_id > i64::MAX as u64
            || self.pull_request_number == 0
            || self.pull_request_number > i64::MAX as u64
            || self.fact_occurred_at != canonical_timestamp(self.fact_occurred_at)
            || self
                .head_commit_sha
                .as_str()
                .bytes()
                .all(|byte| byte == b'0')
            || self.base_repository.provider() != GitProvider::Github
            || self
                .head_repository
                .as_ref()
                .is_some_and(|repository| repository.provider() != GitProvider::Github)
            || matches!(
                self.desired_state,
                PreviewSourceRevisionDesiredState::Active
            ) && self.head_repository.is_none()
        {
            return Err("Preview Source revision projection identity or state is invalid".into());
        }
        validate_repository(&self.base_repository)?;
        if let Some(repository) = &self.head_repository {
            validate_repository(repository)?;
        }
        if !matches!(self.base_branch, GitReference::Branch(_))
            || !matches!(self.head_branch, GitReference::Branch(_))
        {
            return Err("Preview Source revision projection requires exact branches".into());
        }
        GitReference::parse(self.base_branch.kind(), self.base_branch.value())?;
        GitReference::parse(self.head_branch.kind(), self.head_branch.value())?;
        GitCommitSha::parse(self.head_commit_sha.as_str())?;
        Ok(())
    }

    pub(in crate::modules::sources) fn decide(
        &self,
        subscription: &GithubRepositorySubscription,
    ) -> Result<PreviewSourceRevisionProjectionDecision, String> {
        self.validate()?;
        GithubRepositorySubscription::restore(subscription.clone())?;
        if subscription.organization_id != self.organization_id
            || subscription.project_id != self.project_id
            || subscription.environment_id != self.source_environment_id
            || subscription.id != self.source_subscription_id
            || subscription.installation_id != self.installation_id
            || subscription.repository != self.base_repository
            || subscription.branch != self.base_branch
        {
            return Err("Preview lifecycle is outside its Sources subscription authority".into());
        }
        match self.desired_state {
            PreviewSourceRevisionDesiredState::CleanupRequired => {
                Ok(PreviewSourceRevisionProjectionDecision {
                    outcome: PreviewSourceRevisionProjectionOutcome::CleanupRequired,
                    revision: None,
                })
            }
            PreviewSourceRevisionDesiredState::Active if !subscription.is_active() => {
                Ok(PreviewSourceRevisionProjectionDecision {
                    outcome: PreviewSourceRevisionProjectionOutcome::SuppressedInactiveSubscription,
                    revision: None,
                })
            }
            PreviewSourceRevisionDesiredState::Active => {
                let revision = ExternalSourceRevision::accept(NewExternalSourceRevision {
                    organization_id: self.organization_id,
                    project_id: self.project_id,
                    environment_id: self.preview_environment_id,
                    id: SourceRevisionId::new(),
                    repository: self.head_repository.clone().ok_or_else(|| {
                        "active Preview Source projection lost its head repository".to_owned()
                    })?,
                    commit_sha: self.head_commit_sha.clone(),
                    recipe: subscription.recipe.clone(),
                    accepted_at: self.fact_occurred_at,
                })?;
                Ok(PreviewSourceRevisionProjectionDecision {
                    outcome: PreviewSourceRevisionProjectionOutcome::Projected,
                    revision: Some(revision),
                })
            }
        }
    }
}

pub(in crate::modules::sources) struct PreviewSourceRevisionProjectionDecision {
    pub outcome: PreviewSourceRevisionProjectionOutcome,
    pub revision: Option<ExternalSourceRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewSourceRevisionProjectionReceipt {
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
    pub installation_id: GithubInstallationId,
    pub base_repository_identity: String,
    pub base_branch: String,
    pub pull_request_id: u64,
    pub pull_request_number: u64,
    pub fact_digest: Sha256Digest,
    pub fact_occurred_at: DateTime<Utc>,
    pub outcome: PreviewSourceRevisionProjectionOutcome,
    pub source_revision_id: Option<SourceRevisionId>,
}

impl PreviewSourceRevisionProjectionReceipt {
    pub fn from_input(
        input: &ProjectPreviewSourceRevision,
        outcome: PreviewSourceRevisionProjectionOutcome,
        source_revision_id: Option<SourceRevisionId>,
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
            installation_id: input.installation_id,
            base_repository_identity: input.base_repository.identity().to_owned(),
            base_branch: input.base_branch.value().to_owned(),
            pull_request_id: input.pull_request_id,
            pull_request_number: input.pull_request_number,
            fact_digest: input.fact_digest.clone(),
            fact_occurred_at: input.fact_occurred_at,
            outcome,
            source_revision_id,
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn restore(receipt: Self) -> Result<Self, String> {
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        let revision_shape_is_valid = matches!(
            (self.outcome, self.source_revision_id),
            (PreviewSourceRevisionProjectionOutcome::Projected, Some(_))
                | (
                    PreviewSourceRevisionProjectionOutcome::CleanupRequired,
                    None
                )
                | (
                    PreviewSourceRevisionProjectionOutcome::SuppressedInactiveSubscription,
                    None
                )
                | (PreviewSourceRevisionProjectionOutcome::IgnoredStale, None)
        );
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
            || self.base_repository_identity.trim().is_empty()
            || self.base_repository_identity.len() > 2_048
            || self.pull_request_id == 0
            || self.pull_request_id > i64::MAX as u64
            || self.pull_request_number == 0
            || self.pull_request_number > i64::MAX as u64
            || self.fact_occurred_at != canonical_timestamp(self.fact_occurred_at)
            || !revision_shape_is_valid
            || self
                .source_revision_id
                .is_some_and(|id| id.as_uuid().is_nil())
        {
            return Err("Preview Source revision projection receipt is invalid".into());
        }
        GitReference::parse("branch", &self.base_branch)?;
        Ok(())
    }

    pub fn matches_input(&self, input: &ProjectPreviewSourceRevision) -> bool {
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
            && self.installation_id == input.installation_id
            && self.base_repository_identity == input.base_repository.identity()
            && self.base_branch == input.base_branch.value()
            && self.pull_request_id == input.pull_request_id
            && self.pull_request_number == input.pull_request_number
            && self.fact_digest == input.fact_digest
            && self.fact_occurred_at == input.fact_occurred_at
    }

    pub fn has_same_scope_as(&self, input: &ProjectPreviewSourceRevision) -> bool {
        self.organization_id == input.organization_id
            && self.project_id == input.project_id
            && self.source_environment_id == input.source_environment_id
            && self.source_subscription_id == input.source_subscription_id
            && self.preview_id == input.preview_id
            && self.preview_environment_id == input.preview_environment_id
            && self.installation_id == input.installation_id
            && self.base_repository_identity == input.base_repository.identity()
            && self.base_branch == input.base_branch.value()
            && self.pull_request_id == input.pull_request_id
            && self.pull_request_number == input.pull_request_number
    }
}

#[async_trait]
pub trait IPreviewSourceRevisionProjectionPort: Send + Sync {
    async fn project_preview_source_revision(
        &self,
        input: ProjectPreviewSourceRevision,
    ) -> Result<IdempotentWrite<PreviewSourceRevisionProjectionReceipt>, RepositoryError>;
}

pub(in crate::modules::sources) fn lifecycle_event(
    receipt: &PreviewSourceRevisionProjectionReceipt,
    revision: Option<&ExternalSourceRevision>,
) -> Result<DomainEventEnvelope, String> {
    receipt.validate()?;
    let (state, revision) = match receipt.outcome {
        PreviewSourceRevisionProjectionOutcome::Projected => (
            PreviewSourceRevisionLifecycleState::Active,
            Some(revision.ok_or_else(|| {
                "projected Preview Source revision lifecycle is missing its revision".to_owned()
            })?),
        ),
        PreviewSourceRevisionProjectionOutcome::CleanupRequired => {
            (PreviewSourceRevisionLifecycleState::CleanupRequired, None)
        }
        PreviewSourceRevisionProjectionOutcome::SuppressedInactiveSubscription => (
            PreviewSourceRevisionLifecycleState::SuppressedInactiveSubscription,
            None,
        ),
        PreviewSourceRevisionProjectionOutcome::IgnoredStale => {
            return Err("stale Preview Source revision receipts are not publishable".into())
        }
    };
    if let Some(revision) = revision {
        revision.clone().validate()?;
        if Some(revision.id) != receipt.source_revision_id
            || revision.organization_id != receipt.organization_id
            || revision.project_id != receipt.project_id
            || revision.environment_id != receipt.preview_environment_id
        {
            return Err("Preview Source revision lifecycle and revision binding differ".into());
        }
    }
    let fact = PreviewSourceRevisionLifecycleCommittedFact::new(
        receipt.source_pull_request_change_id,
        receipt.organization_id,
        receipt.project_id,
        receipt.source_environment_id,
        receipt.source_subscription_id,
        receipt.preview_id,
        receipt.preview_aggregate_version,
        receipt.preview_environment_id,
        state,
        revision.map(|value| value.id),
        revision.map(|value| value.repository.identity().to_owned()),
        revision.map(|value| value.commit_sha.as_str().to_owned()),
        revision.map(|value| value.recipe_digest.clone()),
        revision.map(|value| value.accepted_at),
    );
    fact.validate()?;
    Ok(DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_EVENT_KEY.into(),
        schema_version: PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_SCHEMA_VERSION,
        scope: a3s_cloud_contracts::CloudScopeRef::Organization {
            organization_id: receipt.organization_id.as_uuid(),
        },
        aggregate_id: receipt.preview_id.as_uuid(),
        aggregate_version: receipt.preview_aggregate_version,
        occurred_at: receipt.fact_occurred_at,
        correlation_id: receipt.correlation_id,
        causation_id: Some(receipt.lifecycle_event_id),
        payload: serde_json::to_value(fact)
            .map_err(|error| format!("Preview Source revision lifecycle is invalid: {error}"))?,
    })
}

fn validate_repository(repository: &GitRepository) -> Result<(), String> {
    let canonical = GitRepository::parse(repository.provider(), repository.canonical_url())?;
    if &canonical != repository {
        return Err("Preview Source repository is not canonical".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_shape_keeps_only_projected_revisions() {
        let input = input();
        assert!(PreviewSourceRevisionProjectionReceipt::from_input(
            &input,
            PreviewSourceRevisionProjectionOutcome::Projected,
            Some(SourceRevisionId::new()),
        )
        .is_ok());
        assert!(PreviewSourceRevisionProjectionReceipt::from_input(
            &input,
            PreviewSourceRevisionProjectionOutcome::CleanupRequired,
            Some(SourceRevisionId::new()),
        )
        .is_err());
    }

    fn input() -> ProjectPreviewSourceRevision {
        ProjectPreviewSourceRevision {
            lifecycle_event_id: Uuid::now_v7(),
            correlation_id: Uuid::now_v7(),
            lifecycle_causation_id: Uuid::now_v7(),
            source_pull_request_change_id: SourcePullRequestChangeId::new(),
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            source_environment_id: EnvironmentId::new(),
            source_subscription_id: SourceSubscriptionId::new(),
            preview_id: PullRequestPreviewId::new(),
            preview_aggregate_version: 1,
            preview_environment_id: EnvironmentId::new(),
            installation_id: GithubInstallationId::parse(42).expect("installation"),
            base_repository: GitRepository::parse(
                GitProvider::Github,
                "https://github.com/a3s-lab/cloud",
            )
            .expect("repository"),
            base_branch: GitReference::parse("branch", "main").expect("branch"),
            head_repository: Some(
                GitRepository::parse(GitProvider::Github, "https://github.com/a3s-lab/cloud")
                    .expect("repository"),
            ),
            head_branch: GitReference::parse("branch", "feature/preview").expect("branch"),
            head_commit_sha: GitCommitSha::parse("a".repeat(40)).expect("commit"),
            pull_request_id: 42,
            pull_request_number: 7,
            desired_state: PreviewSourceRevisionDesiredState::Active,
            fact_digest: Sha256Digest::from_bytes(b"fact"),
            fact_occurred_at: canonical_timestamp(Utc::now()),
        }
    }
}
