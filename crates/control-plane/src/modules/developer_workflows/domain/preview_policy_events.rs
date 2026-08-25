use super::AcceptedPullRequestPreviewPolicyRevision;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, PrincipalId, ProjectId, PullRequestPreviewPolicyRevisionId,
    SourceSubscriptionId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PULL_REQUEST_PREVIEW_POLICY_REVISION_ACCEPTED_EVENT_KEY: &str =
    "developer.pull-request-preview-policy.revision-accepted";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PullRequestPreviewPolicyRevisionAccepted {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub source_environment_id: EnvironmentId,
    pub source_subscription_id: SourceSubscriptionId,
    pub preview_policy_revision_id: PullRequestPreviewPolicyRevisionId,
    pub revision_number: u64,
    pub policy_digest: String,
    pub installation_id: u64,
    pub base_repository_identity: String,
    pub base_branch: String,
    pub owner_principal_id: PrincipalId,
    pub accepted_by: PrincipalId,
}

impl PullRequestPreviewPolicyRevisionAccepted {
    pub fn envelope(
        revision: &AcceptedPullRequestPreviewPolicyRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        revision.validate()?;
        let policy = revision.contract.policy();
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: PULL_REQUEST_PREVIEW_POLICY_REVISION_ACCEPTED_EVENT_KEY.into(),
            schema_version: 1,
            organization_id: revision.organization_id.as_uuid(),
            aggregate_id: revision.source_subscription_id.as_uuid(),
            aggregate_version: revision.revision_number,
            occurred_at: revision.accepted_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: revision.organization_id,
                project_id: revision.project_id,
                source_environment_id: revision.source_environment_id,
                source_subscription_id: revision.source_subscription_id,
                preview_policy_revision_id: revision.id,
                revision_number: revision.revision_number,
                policy_digest: revision.contract.digest().to_string(),
                installation_id: policy.installation_id.as_u64(),
                base_repository_identity: policy.base_repository.identity().into(),
                base_branch: policy.base_branch.as_str().into(),
                owner_principal_id: policy.owner_principal_id,
                accepted_by: revision.accepted_by,
            })
            .map_err(|error| format!("Preview policy revision event is invalid: {error}"))?,
        })
    }
}
