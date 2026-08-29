use crate::modules::identity::application::{
    PlatformRoleBindingMutationResult, PlatformRolePolicyMutationResult,
    TenantSupportGrantApprovalMutationResult, TenantSupportGrantMutationResult,
    TenantSupportGrantProposalMutationResult,
};
use crate::modules::identity::domain::entities::{
    AcceptedPlatformRolePolicyRevision, PlatformRoleBinding, TenantSupportGrant,
    TenantSupportGrantApproval, TenantSupportGrantApprovalOutcome, TenantSupportGrantProposal,
};
use crate::modules::identity::domain::repositories::TenantSupportGrantRecord;
use crate::modules::shared_kernel::domain::{DecisionEvidenceRef, ScopeContext};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformRolePermissionResponse {
    pub role: String,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformRolePolicyResponse {
    pub id: Uuid,
    pub installation_id: Uuid,
    pub policy_id: Uuid,
    pub revision_number: u64,
    pub canonical_acl: String,
    pub digest: String,
    pub role_permissions: Vec<PlatformRolePermissionResponse>,
    pub accepted_by: Uuid,
    pub accepted_at: DateTime<Utc>,
}

impl From<AcceptedPlatformRolePolicyRevision> for PlatformRolePolicyResponse {
    fn from(policy: AcceptedPlatformRolePolicyRevision) -> Self {
        let role_permissions = policy
            .contract
            .spec()
            .role_permissions
            .iter()
            .map(|entry| PlatformRolePermissionResponse {
                role: entry.role.as_str().into(),
                permissions: entry
                    .permissions
                    .iter()
                    .map(|permission| permission.as_str().into())
                    .collect(),
            })
            .collect();
        Self {
            id: policy.id.as_uuid(),
            installation_id: policy.installation_id.as_uuid(),
            policy_id: policy.policy_id.as_uuid(),
            revision_number: policy.revision_number,
            canonical_acl: policy.contract.canonical_acl().into(),
            digest: policy.contract.digest().as_str().into(),
            role_permissions,
            accepted_by: policy.accepted_by.as_uuid(),
            accepted_at: policy.accepted_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformRolePolicyMutationResponse {
    #[serde(flatten)]
    pub policy: PlatformRolePolicyResponse,
    pub replayed: bool,
}

impl From<PlatformRolePolicyMutationResult> for PlatformRolePolicyMutationResponse {
    fn from(result: PlatformRolePolicyMutationResult) -> Self {
        Self {
            policy: result.policy.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformRoleBindingResponse {
    pub id: Uuid,
    pub installation_id: Uuid,
    pub principal_id: Uuid,
    pub role: String,
    pub aggregate_version: u64,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<PlatformRoleBinding> for PlatformRoleBindingResponse {
    fn from(binding: PlatformRoleBinding) -> Self {
        Self {
            id: binding.id.as_uuid(),
            installation_id: binding.installation_id.as_uuid(),
            principal_id: binding.principal_id.as_uuid(),
            role: binding.role.as_str().into(),
            aggregate_version: binding.aggregate_version,
            created_by: binding.created_by.as_uuid(),
            updated_by: binding.updated_by.as_uuid(),
            created_at: binding.created_at,
            updated_at: binding.updated_at,
            revoked_at: binding.revoked_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformRoleBindingMutationResponse {
    #[serde(flatten)]
    pub binding: PlatformRoleBindingResponse,
    pub replayed: bool,
}

impl From<PlatformRoleBindingMutationResult> for PlatformRoleBindingMutationResponse {
    fn from(result: PlatformRoleBindingMutationResult) -> Self {
        Self {
            binding: result.binding.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantSupportScopeResponse {
    pub kind: String,
    pub installation_id: Uuid,
    pub organization_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub environment_id: Option<Uuid>,
}

impl From<ScopeContext> for TenantSupportScopeResponse {
    fn from(scope: ScopeContext) -> Self {
        Self {
            kind: scope.kind().into(),
            installation_id: scope.installation_id().as_uuid(),
            organization_id: scope.organization_id().map(|value| value.as_uuid()),
            project_id: scope.project_id().map(|value| value.as_uuid()),
            environment_id: scope.environment_id().map(|value| value.as_uuid()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionEvidenceResponse {
    pub id: String,
    pub digest: String,
}

impl From<DecisionEvidenceRef> for DecisionEvidenceResponse {
    fn from(evidence: DecisionEvidenceRef) -> Self {
        Self {
            id: evidence.id,
            digest: evidence.digest.as_str().into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantSupportGrantProposalResponse {
    pub id: Uuid,
    pub principal_id: Uuid,
    pub scope: TenantSupportScopeResponse,
    pub permissions: Vec<String>,
    pub case_reference: String,
    pub justification_digest: String,
    pub mode: String,
    pub approval_requirement: String,
    pub approver_ids: Vec<Uuid>,
    pub tenant_notification: String,
    pub security_alert_required: bool,
    pub post_incident_review_required: bool,
    pub starts_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub canonical_acl: String,
    pub contract_digest: String,
    pub requested_by: Uuid,
    pub authentication: DecisionEvidenceResponse,
    pub requested_at: DateTime<Utc>,
}

impl From<TenantSupportGrantProposal> for TenantSupportGrantProposalResponse {
    fn from(proposal: TenantSupportGrantProposal) -> Self {
        let spec = proposal.contract.spec();
        Self {
            id: proposal.id.as_uuid(),
            principal_id: spec.principal_id.as_uuid(),
            scope: spec.scope.into(),
            permissions: spec
                .permissions
                .iter()
                .map(|permission| permission.as_str().into())
                .collect(),
            case_reference: spec.case_reference.clone(),
            justification_digest: spec.justification_digest.as_str().into(),
            mode: spec.mode.as_str().into(),
            approval_requirement: spec.approval_requirement.as_str().into(),
            approver_ids: spec
                .approver_ids
                .iter()
                .map(|value| value.as_uuid())
                .collect(),
            tenant_notification: spec.tenant_notification.as_str().into(),
            security_alert_required: spec.security_alert_required,
            post_incident_review_required: spec.post_incident_review_required,
            starts_at: spec.starts_at,
            expires_at: spec.expires_at,
            canonical_acl: proposal.contract.canonical_acl().into(),
            contract_digest: proposal.contract.digest().as_str().into(),
            requested_by: proposal.requested_by.as_uuid(),
            authentication: proposal.authentication.into(),
            requested_at: proposal.requested_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantSupportGrantApprovalResponse {
    pub grant_id: Uuid,
    pub contract_digest: String,
    pub approver_id: Uuid,
    pub authentication: DecisionEvidenceResponse,
    pub policy_revision_id: Uuid,
    pub policy_digest: String,
    pub binding_id: Uuid,
    pub binding_version: u64,
    pub approved_at: DateTime<Utc>,
    pub digest: String,
}

impl From<TenantSupportGrantApproval> for TenantSupportGrantApprovalResponse {
    fn from(approval: TenantSupportGrantApproval) -> Self {
        Self {
            grant_id: approval.grant_id.as_uuid(),
            contract_digest: approval.contract_digest.as_str().into(),
            approver_id: approval.approver_id.as_uuid(),
            authentication: approval.authentication.into(),
            policy_revision_id: approval.policy_revision_id.as_uuid(),
            policy_digest: approval.policy_digest.as_str().into(),
            binding_id: approval.binding_id.as_uuid(),
            binding_version: approval.binding_version,
            approved_at: approval.approved_at,
            digest: approval.digest.as_str().into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantSupportGrantLifecycleResponse {
    pub id: Uuid,
    pub aggregate_version: u64,
    pub revocation_generation: u64,
    pub accepted_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_by: Option<Uuid>,
}

impl From<TenantSupportGrant> for TenantSupportGrantLifecycleResponse {
    fn from(grant: TenantSupportGrant) -> Self {
        Self {
            id: grant.id.as_uuid(),
            aggregate_version: grant.aggregate_version,
            revocation_generation: grant.revocation_generation,
            accepted_at: grant.accepted_at,
            revoked_at: grant.revoked_at,
            revoked_by: grant.revoked_by.map(|value| value.as_uuid()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantSupportGrantResponse {
    pub proposal: TenantSupportGrantProposalResponse,
    pub approvals: Vec<TenantSupportGrantApprovalResponse>,
    pub grant: Option<TenantSupportGrantLifecycleResponse>,
}

impl From<TenantSupportGrantRecord> for TenantSupportGrantResponse {
    fn from(record: TenantSupportGrantRecord) -> Self {
        Self {
            proposal: record.proposal.into(),
            approvals: record.approvals.into_iter().map(Into::into).collect(),
            grant: record.grant.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantSupportGrantProposalMutationResponse {
    pub proposal: TenantSupportGrantProposalResponse,
    pub replayed: bool,
}

impl From<TenantSupportGrantProposalMutationResult> for TenantSupportGrantProposalMutationResponse {
    fn from(result: TenantSupportGrantProposalMutationResult) -> Self {
        Self {
            proposal: result.proposal.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantSupportGrantApprovalOutcomeResponse {
    pub proposal: TenantSupportGrantProposalResponse,
    pub approval: TenantSupportGrantApprovalResponse,
    pub grant: Option<TenantSupportGrantLifecycleResponse>,
}

impl From<TenantSupportGrantApprovalOutcome> for TenantSupportGrantApprovalOutcomeResponse {
    fn from(outcome: TenantSupportGrantApprovalOutcome) -> Self {
        Self {
            proposal: outcome.proposal.into(),
            approval: outcome.approval.into(),
            grant: outcome.grant.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantSupportGrantApprovalMutationResponse {
    pub outcome: TenantSupportGrantApprovalOutcomeResponse,
    pub replayed: bool,
}

impl From<TenantSupportGrantApprovalMutationResult> for TenantSupportGrantApprovalMutationResponse {
    fn from(result: TenantSupportGrantApprovalMutationResult) -> Self {
        Self {
            outcome: result.outcome.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantSupportGrantMutationResponse {
    pub grant: TenantSupportGrantLifecycleResponse,
    pub replayed: bool,
}

impl From<TenantSupportGrantMutationResult> for TenantSupportGrantMutationResponse {
    fn from(result: TenantSupportGrantMutationResult) -> Self {
        Self {
            grant: result.grant.into(),
            replayed: result.replayed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::{
        PlatformRole, PlatformRolePolicyContract,
    };
    use crate::modules::shared_kernel::domain::{
        InstallationId, PlatformRoleBindingId, PlatformRolePolicyId, PrincipalId,
    };

    #[test]
    fn platform_management_responses_are_structured_camel_case_contracts() {
        let installation_id = InstallationId::new();
        let actor = PrincipalId::new();
        let policy = AcceptedPlatformRolePolicyRevision::accept(
            PlatformRolePolicyContract::baseline(installation_id, PlatformRolePolicyId::new())
                .expect("policy contract"),
            1,
            actor,
            Utc::now(),
        )
        .expect("policy");
        let binding = PlatformRoleBinding::create(
            PlatformRoleBindingId::new(),
            installation_id,
            PrincipalId::new(),
            PlatformRole::PlatformOperator,
            &policy,
            actor,
            Utc::now(),
        )
        .expect("binding");

        let policy_json =
            serde_json::to_value(PlatformRolePolicyResponse::from(policy)).expect("policy JSON");
        assert!(policy_json.get("canonicalAcl").is_some());
        assert!(policy_json.get("rolePermissions").is_some());
        assert!(policy_json.get("canonical_acl").is_none());
        let binding_json =
            serde_json::to_value(PlatformRoleBindingResponse::from(binding)).expect("binding JSON");
        assert_eq!(binding_json["role"], "platform_operator");
        assert!(binding_json.get("aggregateVersion").is_some());
        assert!(binding_json.get("aggregate_version").is_none());
    }
}
