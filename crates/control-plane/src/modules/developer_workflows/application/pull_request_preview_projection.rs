use crate::modules::developer_workflows::domain::{
    reconcile_pull_request_preview, CommitPullRequestPreviewProjection,
    IPullRequestPreviewPolicyRepository, IPullRequestPreviewProjectionRepository,
    PreviewReconcileOutcome, PullRequestChange, PullRequestPreview,
    PullRequestPreviewPolicyAuthority, PullRequestPreviewProjectionOutcome,
    PullRequestPreviewProjectionReceipt, PullRequestPreviewVersion,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, IdempotentWrite, OrganizationId, ProjectId,
    RepositoryError, Sha256Digest, SourcePullRequestChangeId, SourceSubscriptionId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ProjectCommittedPullRequestChange {
    pub source_pull_request_change_id: SourcePullRequestChangeId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub source_environment_id: EnvironmentId,
    pub source_subscription_id: SourceSubscriptionId,
    pub change: PullRequestChange,
    pub fact_digest: Sha256Digest,
    pub fact_occurred_at: DateTime<Utc>,
}

impl ProjectCommittedPullRequestChange {
    pub fn validate(&self) -> Result<(), String> {
        self.change.validate()?;
        if self.source_pull_request_change_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.source_environment_id.as_uuid().is_nil()
            || self.source_subscription_id.as_uuid().is_nil()
            || self.fact_digest != Sha256Digest::parse(self.fact_digest.as_str())?
            || self.fact_occurred_at != canonical_timestamp(self.fact_occurred_at)
        {
            return Err("committed pull-request change projection input is invalid".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait IPullRequestPreviewProjectionPort: Send + Sync {
    async fn project_committed_change(
        &self,
        input: ProjectCommittedPullRequestChange,
    ) -> Result<IdempotentWrite<PullRequestPreviewProjectionReceipt>, RepositoryError>;
}

/// Application orchestration for the Developer Workflows-owned local
/// projection. It selects policy by the fact's event time, delegates lifecycle
/// ordering to the pure reducer, and commits state plus receipt atomically.
pub struct PullRequestPreviewProjectionService {
    policies: Arc<dyn IPullRequestPreviewPolicyRepository>,
    previews: Arc<dyn IPullRequestPreviewProjectionRepository>,
}

impl PullRequestPreviewProjectionService {
    pub fn new(
        policies: Arc<dyn IPullRequestPreviewPolicyRepository>,
        previews: Arc<dyn IPullRequestPreviewProjectionRepository>,
    ) -> Self {
        Self { policies, previews }
    }

    async fn execute(
        &self,
        input: ProjectCommittedPullRequestChange,
    ) -> Result<IdempotentWrite<PullRequestPreviewProjectionReceipt>, RepositoryError> {
        input.validate().map_err(invalid_input)?;
        if let Some(receipt) = self
            .previews
            .find_receipt(input.organization_id, input.source_pull_request_change_id)
            .await?
        {
            return replay(&input, receipt);
        }

        let current = self
            .previews
            .find_preview(
                input.organization_id,
                input.project_id,
                input.source_environment_id,
                input.source_subscription_id,
                input.change.pull_request_id,
            )
            .await?;
        if let Some(preview) = &current {
            validate_current_binding(preview, &input).map_err(invalid_input)?;
        }

        let authority = match current.as_ref() {
            Some(preview) => Some(preview.policy_authority.clone()),
            None => self
                .policies
                .find_effective_at(
                    input.organization_id,
                    input.project_id,
                    input.source_environment_id,
                    input.source_subscription_id,
                    input.fact_occurred_at,
                )
                .await?
                .map(|revision| {
                    revision
                        .preview_authority()
                        .map_err(invalid_input)
                        .and_then(|authority| {
                            validate_authority_binding(&authority, &input)
                                .map_err(invalid_input)?;
                            Ok(authority)
                        })
                })
                .transpose()?,
        };

        let expected_preview = current.as_ref().map(|preview| PullRequestPreviewVersion {
            id: preview.id,
            aggregate_version: preview.aggregate_version,
        });
        let Some(authority) = authority else {
            let receipt = receipt(
                &input,
                PullRequestPreviewProjectionOutcome::NoApplicablePolicy,
                None,
                None,
            );
            return self
                .previews
                .commit_projection(CommitPullRequestPreviewProjection {
                    receipt,
                    expected_preview,
                    preview: None,
                })
                .await;
        };

        let reconciliation =
            reconcile_pull_request_preview(&authority, current.as_ref(), &input.change)
                .map_err(invalid_input)?;
        let outcome = projection_outcome(reconciliation.outcome);
        let projected_preview = reconciliation.preview;
        let preview_version = projected_preview
            .as_ref()
            .map(|preview| PullRequestPreviewVersion {
                id: preview.id,
                aggregate_version: preview.aggregate_version,
            });
        let mutation = projected_preview
            .as_ref()
            .filter(|preview| current.as_ref() != Some(*preview))
            .cloned();
        let receipt = receipt(
            &input,
            outcome,
            Some(authority.revision_id),
            preview_version,
        );
        self.previews
            .commit_projection(CommitPullRequestPreviewProjection {
                receipt,
                expected_preview,
                preview: mutation,
            })
            .await
    }
}

#[async_trait]
impl IPullRequestPreviewProjectionPort for PullRequestPreviewProjectionService {
    async fn project_committed_change(
        &self,
        input: ProjectCommittedPullRequestChange,
    ) -> Result<IdempotentWrite<PullRequestPreviewProjectionReceipt>, RepositoryError> {
        self.execute(input).await
    }
}

fn replay(
    input: &ProjectCommittedPullRequestChange,
    receipt: PullRequestPreviewProjectionReceipt,
) -> Result<IdempotentWrite<PullRequestPreviewProjectionReceipt>, RepositoryError> {
    if !receipt.matches_fact(
        input.organization_id,
        input.project_id,
        input.source_environment_id,
        input.source_subscription_id,
        input.change.pull_request_id,
        input.change.pull_request_number,
        &input.fact_digest,
        input.fact_occurred_at,
    ) {
        return Err(RepositoryError::Conflict(
            "Sources pull-request fact ID changed content or owner binding".into(),
        ));
    }
    Ok(IdempotentWrite {
        value: receipt,
        replayed: true,
    })
}

fn validate_current_binding(
    preview: &PullRequestPreview,
    input: &ProjectCommittedPullRequestChange,
) -> Result<(), String> {
    preview.validate()?;
    validate_authority_binding(&preview.policy_authority, input)?;
    if preview.pull_request_id != input.change.pull_request_id
        || preview.pull_request_number != input.change.pull_request_number
    {
        return Err("committed pull-request fact does not match its current Preview".into());
    }
    Ok(())
}

fn validate_authority_binding(
    authority: &PullRequestPreviewPolicyAuthority,
    input: &ProjectCommittedPullRequestChange,
) -> Result<(), String> {
    authority.validate()?;
    let policy = &authority.policy;
    if policy.organization_id != input.organization_id
        || policy.project_id != input.project_id
        || authority.source_environment_id != input.source_environment_id
        || policy.source_subscription_id != input.source_subscription_id
    {
        return Err("committed pull-request fact is outside its Preview Policy authority".into());
    }
    Ok(())
}

fn receipt(
    input: &ProjectCommittedPullRequestChange,
    outcome: PullRequestPreviewProjectionOutcome,
    policy_revision_id: Option<
        crate::modules::shared_kernel::domain::PullRequestPreviewPolicyRevisionId,
    >,
    preview_version: Option<PullRequestPreviewVersion>,
) -> PullRequestPreviewProjectionReceipt {
    PullRequestPreviewProjectionReceipt {
        source_pull_request_change_id: input.source_pull_request_change_id,
        organization_id: input.organization_id,
        project_id: input.project_id,
        source_environment_id: input.source_environment_id,
        source_subscription_id: input.source_subscription_id,
        pull_request_id: input.change.pull_request_id,
        pull_request_number: input.change.pull_request_number,
        fact_digest: input.fact_digest.clone(),
        fact_occurred_at: input.fact_occurred_at,
        policy_revision_id,
        preview_id: preview_version.map(|preview| preview.id),
        preview_aggregate_version: preview_version.map(|preview| preview.aggregate_version),
        outcome,
    }
}

const fn projection_outcome(
    outcome: PreviewReconcileOutcome,
) -> PullRequestPreviewProjectionOutcome {
    match outcome {
        PreviewReconcileOutcome::Created => PullRequestPreviewProjectionOutcome::Created,
        PreviewReconcileOutcome::Updated => PullRequestPreviewProjectionOutcome::Updated,
        PreviewReconcileOutcome::Reactivated => PullRequestPreviewProjectionOutcome::Reactivated,
        PreviewReconcileOutcome::CleanupRequired => {
            PullRequestPreviewProjectionOutcome::CleanupRequired
        }
        PreviewReconcileOutcome::ForkDenied => PullRequestPreviewProjectionOutcome::ForkDenied,
        PreviewReconcileOutcome::IgnoredDuplicate => {
            PullRequestPreviewProjectionOutcome::IgnoredDuplicate
        }
        PreviewReconcileOutcome::IgnoredStale => PullRequestPreviewProjectionOutcome::IgnoredStale,
    }
}

fn invalid_input(error: String) -> RepositoryError {
    RepositoryError::Storage(format!(
        "Developer Workflows pull-request projection is invalid: {error}"
    ))
}
