use super::{
    AcceptedPullRequestPreviewPolicyRevision, PullRequestPreviewPolicyRevisionAccepted,
    PULL_REQUEST_PREVIEW_POLICY_REVISION_ACCEPTED_EVENT_KEY,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, ProjectId,
    PullRequestPreviewPolicyRevisionId, RepositoryError, SourceSubscriptionId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_PULL_REQUEST_PREVIEW_POLICY_REVISIONS_PAGE: usize = 100;

#[derive(Debug, Clone)]
pub struct AcceptPullRequestPreviewPolicyRevisionWrite {
    pub revision: AcceptedPullRequestPreviewPolicyRevision,
    pub expected_previous_revision_id: Option<PullRequestPreviewPolicyRevisionId>,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl AcceptPullRequestPreviewPolicyRevisionWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.revision.validate()?;
        self.idempotency.validate()?;
        let expected_previous_shape = match self.revision.revision_number {
            1 => self.expected_previous_revision_id.is_none(),
            _ => self
                .expected_previous_revision_id
                .is_some_and(|id| !id.as_uuid().is_nil() && id != self.revision.id),
        };
        if !expected_previous_shape
            || self.actor_principal_id.as_uuid().is_nil()
            || self.actor_principal_id != self.revision.accepted_by
            || self.request_id.is_nil()
            || self.event.event_id.is_nil()
            || self.event.event_key != PULL_REQUEST_PREVIEW_POLICY_REVISION_ACCEPTED_EVENT_KEY
            || self.event.schema_version != 1
            || self.event.organization_id() != Some(self.revision.organization_id.as_uuid())
            || self.event.aggregate_id != self.revision.source_subscription_id.as_uuid()
            || self.event.aggregate_version != self.revision.revision_number
            || self.event.occurred_at != self.revision.accepted_at
            || self.event.correlation_id != self.request_id
            || self.event.causation_id.is_some()
        {
            return Err("Preview policy revision write and event identity are inconsistent".into());
        }
        let payload: PullRequestPreviewPolicyRevisionAccepted =
            serde_json::from_value(self.event.payload.clone())
                .map_err(|error| format!("Preview policy event payload is invalid: {error}"))?;
        let policy = self.revision.contract.policy();
        if payload.organization_id != self.revision.organization_id
            || payload.project_id != self.revision.project_id
            || payload.source_environment_id != self.revision.source_environment_id
            || payload.source_subscription_id != self.revision.source_subscription_id
            || payload.preview_policy_revision_id != self.revision.id
            || payload.revision_number != self.revision.revision_number
            || payload.policy_digest != self.revision.contract.digest().as_str()
            || payload.installation_id != policy.installation_id.as_u64()
            || payload.base_repository_identity != policy.base_repository.identity()
            || payload.base_branch != policy.base_branch.as_str()
            || payload.owner_principal_id != policy.owner_principal_id
            || payload.accepted_by != self.revision.accepted_by
        {
            return Err("Preview policy revision event payload is inconsistent".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PreviewPolicyRevisionWriteReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub source_environment_id: EnvironmentId,
    pub source_subscription_id: SourceSubscriptionId,
    pub preview_policy_revision_id: PullRequestPreviewPolicyRevisionId,
}

impl From<&AcceptedPullRequestPreviewPolicyRevision> for PreviewPolicyRevisionWriteReference {
    fn from(revision: &AcceptedPullRequestPreviewPolicyRevision) -> Self {
        Self {
            organization_id: revision.organization_id,
            project_id: revision.project_id,
            source_environment_id: revision.source_environment_id,
            source_subscription_id: revision.source_subscription_id,
            preview_policy_revision_id: revision.id,
        }
    }
}

#[async_trait]
pub trait IPullRequestPreviewPolicyRepository: Send + Sync {
    async fn replay_acceptance(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError>;

    async fn accept(
        &self,
        write: AcceptPullRequestPreviewPolicyRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError>;

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        revision_id: PullRequestPreviewPolicyRevisionId,
    ) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError>;

    async fn find_current(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
    ) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError>;

    /// Selects the last revision accepted no later than the owner-published
    /// fact time. Relay delay must not let a later policy rewrite history.
    async fn find_effective_at(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        fact_occurred_at: DateTime<Utc>,
    ) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError>;

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        limit: usize,
    ) -> Result<Vec<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError>;
}
