use crate::modules::developer_workflows::{
    AcceptPullRequestPreviewPolicyResult, AcceptedPullRequestPreviewPolicyRevision, PreviewQuota,
    PullRequestPreview, PullRequestPreviewPolicy, PullRequestPreviewStatus,
};
use crate::modules::sources::published::GitRepository;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptPullRequestPreviewPolicyRequest {
    pub source_subscription_id: Uuid,
    pub policy_acl: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewGitRepositoryResponse {
    pub provider: String,
    pub canonical_url: String,
}

impl From<&GitRepository> for PreviewGitRepositoryResponse {
    fn from(repository: &GitRepository) -> Self {
        Self {
            provider: repository.provider().as_str().into(),
            canonical_url: repository.canonical_url().into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewQuotaResponse {
    pub maximum_workloads: u16,
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub ephemeral_storage_bytes: u64,
}

impl From<&PreviewQuota> for PreviewQuotaResponse {
    fn from(quota: &PreviewQuota) -> Self {
        Self {
            maximum_workloads: quota.maximum_workloads,
            cpu_millis: quota.cpu_millis,
            memory_bytes: quota.memory_bytes,
            ephemeral_storage_bytes: quota.ephemeral_storage_bytes,
        }
    }
}

/// Behavioral Preview policy projection. Source credentials, webhook
/// signatures, delivery payloads, and runtime deployment state are not part of
/// this public contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestPreviewPolicyResponse {
    pub owner_principal_id: Uuid,
    pub installation_id: u64,
    pub base_repository: PreviewGitRepositoryResponse,
    pub base_branch: String,
    pub lifetime_seconds: u32,
    pub maximum_active_previews: u16,
    pub fork_policy: String,
    pub allow_protected_secrets_for_trusted_sources: bool,
    pub quota: PreviewQuotaResponse,
}

impl From<&PullRequestPreviewPolicy> for PullRequestPreviewPolicyResponse {
    fn from(policy: &PullRequestPreviewPolicy) -> Self {
        Self {
            owner_principal_id: policy.owner_principal_id.as_uuid(),
            installation_id: policy.installation_id.as_u64(),
            base_repository: (&policy.base_repository).into(),
            base_branch: policy.base_branch.as_str().into(),
            lifetime_seconds: policy.lifetime_seconds,
            maximum_active_previews: policy.maximum_active_previews,
            fork_policy: policy.fork_policy.as_str().into(),
            allow_protected_secrets_for_trusted_sources: policy
                .allow_protected_secrets_for_trusted_sources,
            quota: (&policy.quota).into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedPullRequestPreviewPolicyRevisionResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub source_environment_id: Uuid,
    pub source_subscription_id: Uuid,
    pub pull_request_preview_policy_revision_id: Uuid,
    pub revision_number: u64,
    pub contract_schema: String,
    pub contract_acl: String,
    pub contract_digest: String,
    pub policy: PullRequestPreviewPolicyResponse,
    pub accepted_by: Uuid,
    pub accepted_at: DateTime<Utc>,
}

impl From<AcceptedPullRequestPreviewPolicyRevision>
    for AcceptedPullRequestPreviewPolicyRevisionResponse
{
    fn from(revision: AcceptedPullRequestPreviewPolicyRevision) -> Self {
        Self {
            organization_id: revision.organization_id.as_uuid(),
            project_id: revision.project_id.as_uuid(),
            source_environment_id: revision.source_environment_id.as_uuid(),
            source_subscription_id: revision.source_subscription_id.as_uuid(),
            pull_request_preview_policy_revision_id: revision.id.as_uuid(),
            revision_number: revision.revision_number,
            contract_schema: revision.contract.schema().into(),
            contract_acl: revision.contract.canonical_acl().into(),
            contract_digest: revision.contract.digest().as_str().into(),
            policy: revision.contract.policy().into(),
            accepted_by: revision.accepted_by.as_uuid(),
            accepted_at: revision.accepted_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestPreviewPolicyMutationResponse {
    pub preview_policy_revision: AcceptedPullRequestPreviewPolicyRevisionResponse,
    pub replayed: bool,
}

impl From<AcceptPullRequestPreviewPolicyResult> for PullRequestPreviewPolicyMutationResponse {
    fn from(result: AcceptPullRequestPreviewPolicyResult) -> Self {
        Self {
            preview_policy_revision: result.revision.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestPreviewResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub source_environment_id: Uuid,
    pub source_subscription_id: Uuid,
    pub preview_id: Uuid,
    pub environment_id: Uuid,
    pub environment_name: String,
    pub pull_request_id: u64,
    pub pull_request_number: u64,
    pub policy_revision_id: Uuid,
    pub policy_revision_number: u64,
    pub policy_accepted_at: DateTime<Utc>,
    pub policy: PullRequestPreviewPolicyResponse,
    pub head_repository: Option<PreviewGitRepositoryResponse>,
    pub head_branch: String,
    pub head_commit_sha: String,
    pub provider_created_at: DateTime<Utc>,
    pub last_provider_updated_at: DateTime<Utc>,
    pub last_change_kind: String,
    pub last_merged: bool,
    pub expires_at: DateTime<Utc>,
    pub status: String,
    pub cleanup_reason: Option<String>,
    pub cleanup_requested_at: Option<DateTime<Utc>>,
    pub aggregate_version: u64,
    pub is_fork: bool,
    pub protected_secrets_eligible: bool,
}

impl From<PullRequestPreview> for PullRequestPreviewResponse {
    fn from(preview: PullRequestPreview) -> Self {
        let (status, cleanup_reason, cleanup_requested_at) = match &preview.status {
            PullRequestPreviewStatus::Active => ("active".into(), None, None),
            PullRequestPreviewStatus::CleanupRequired {
                reason,
                requested_at,
            } => (
                "cleanup_required".into(),
                Some(reason.as_str().into()),
                Some(*requested_at),
            ),
        };
        let authority = &preview.policy_authority;
        Self {
            organization_id: authority.policy.organization_id.as_uuid(),
            project_id: authority.policy.project_id.as_uuid(),
            source_environment_id: authority.source_environment_id.as_uuid(),
            source_subscription_id: authority.policy.source_subscription_id.as_uuid(),
            preview_id: preview.id.as_uuid(),
            environment_id: preview.environment_id.as_uuid(),
            environment_name: preview.environment_name(),
            pull_request_id: preview.pull_request_id,
            pull_request_number: preview.pull_request_number,
            policy_revision_id: authority.revision_id.as_uuid(),
            policy_revision_number: authority.revision_number,
            policy_accepted_at: authority.accepted_at,
            policy: (&authority.policy).into(),
            head_repository: preview.head_repository.as_ref().map(Into::into),
            head_branch: preview.head_branch.as_str().into(),
            head_commit_sha: preview.head_commit_sha.as_str().into(),
            provider_created_at: preview.provider_created_at,
            last_provider_updated_at: preview.last_provider_updated_at,
            last_change_kind: preview.last_change_kind.as_str().into(),
            last_merged: preview.last_merged,
            expires_at: preview.expires_at,
            status,
            cleanup_reason,
            cleanup_requested_at,
            aggregate_version: preview.aggregate_version,
            is_fork: preview.is_fork(),
            protected_secrets_eligible: preview.protected_secrets_eligible(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::developer_workflows::{
        AcceptedPullRequestPreviewPolicyRevision, PullRequestPreviewPolicyContract,
    };
    use crate::modules::shared_kernel::domain::{EnvironmentId, PrincipalId};

    const POLICY_FIXTURE: &str =
        include_str!("../../../../../../contracts/p0.3/pull-request-preview-policy.acl");

    #[test]
    fn preview_policy_acceptance_request_is_closed_and_acl_only() {
        let source_subscription_id = Uuid::now_v7();
        let request: AcceptPullRequestPreviewPolicyRequest =
            serde_json::from_value(serde_json::json!({
                "sourceSubscriptionId": source_subscription_id,
                "policyAcl": POLICY_FIXTURE
            }))
            .expect("closed Preview Policy request");
        assert_eq!(request.source_subscription_id, source_subscription_id);
        assert_eq!(request.policy_acl, POLICY_FIXTURE);
        assert!(
            serde_json::from_value::<AcceptPullRequestPreviewPolicyRequest>(serde_json::json!({
                "sourceSubscriptionId": source_subscription_id,
                "policyAcl": POLICY_FIXTURE,
                "policy": {}
            }))
            .is_err()
        );
    }

    #[test]
    fn preview_policy_response_preserves_canonical_acl_without_sensitive_mechanisms() {
        let contract =
            PullRequestPreviewPolicyContract::parse_acl(POLICY_FIXTURE).expect("policy ACL");
        let revision = AcceptedPullRequestPreviewPolicyRevision::accept(
            EnvironmentId::new(),
            contract,
            1,
            PrincipalId::new(),
            Utc::now(),
        )
        .expect("accepted Preview Policy");
        let response = AcceptedPullRequestPreviewPolicyRevisionResponse::from(revision);
        assert_eq!(response.contract_acl, POLICY_FIXTURE);
        assert_eq!(response.policy.installation_id, 42);
        assert_eq!(response.policy.base_repository.provider, "github");

        let json = serde_json::to_value(response).expect("response JSON");
        let encoded = json.to_string();
        for forbidden in [
            "credential",
            "webhookSecret",
            "signature",
            "deliveryBody",
            "providerToken",
        ] {
            assert!(!encoded.contains(forbidden));
        }
    }
}
