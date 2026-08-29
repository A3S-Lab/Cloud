use crate::modules::identity::domain::entities::{
    AcceptedPlatformRolePolicyRevision, PlatformRoleBinding, TenantSupportGrant,
};
use crate::modules::identity::domain::value_objects::{
    PlatformPermission, TenantSupportGrantContract,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, DecisionEvidenceRef,
    PlatformRoleBindingId, PlatformRolePolicyRevisionId, PrincipalId, Sha256Digest,
    TenantSupportGrantId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantSupportGrantProposal {
    pub id: TenantSupportGrantId,
    pub contract: TenantSupportGrantContract,
    pub requested_by: PrincipalId,
    pub authentication: DecisionEvidenceRef,
    pub requested_at: DateTime<Utc>,
}

impl TenantSupportGrantProposal {
    pub fn propose(
        contract: TenantSupportGrantContract,
        requested_by: PrincipalId,
        authentication: DecisionEvidenceRef,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            id: contract.spec().grant_id,
            contract,
            requested_by,
            authentication,
            requested_at: canonical_timestamp(requested_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.contract.validate()?;
        self.authentication.validate()?;
        if self.id.as_uuid().is_nil()
            || self.id != self.contract.spec().grant_id
            || self.requested_by.as_uuid().is_nil()
            || self
                .contract
                .spec()
                .approver_ids
                .contains(&self.requested_by)
            || self.requested_at != canonical_timestamp(self.requested_at)
            || self.requested_at >= self.contract.spec().expires_at
        {
            return Err("tenant support grant proposal is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantSupportGrantApproval {
    pub grant_id: TenantSupportGrantId,
    pub contract_digest: Sha256Digest,
    pub approver_id: PrincipalId,
    pub authentication: DecisionEvidenceRef,
    pub policy_revision_id: PlatformRolePolicyRevisionId,
    pub policy_digest: Sha256Digest,
    pub binding_id: PlatformRoleBindingId,
    pub binding_version: u64,
    pub approved_at: DateTime<Utc>,
    pub digest: Sha256Digest,
}

#[derive(Serialize)]
struct TenantSupportGrantApprovalDigestContent<'a> {
    grant_id: TenantSupportGrantId,
    contract_digest: &'a Sha256Digest,
    approver_id: PrincipalId,
    authentication: &'a DecisionEvidenceRef,
    policy_revision_id: PlatformRolePolicyRevisionId,
    policy_digest: &'a Sha256Digest,
    binding_id: PlatformRoleBindingId,
    binding_version: u64,
    approved_at: DateTime<Utc>,
}

impl TenantSupportGrantApproval {
    pub fn record(
        proposal: &TenantSupportGrantProposal,
        approver_id: PrincipalId,
        authentication: DecisionEvidenceRef,
        policy: &AcceptedPlatformRolePolicyRevision,
        binding: &PlatformRoleBinding,
        approved_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        proposal.validate()?;
        policy.validate()?;
        binding.validate_against_policy(policy)?;
        if !binding.is_active()
            || binding.principal_id != approver_id
            || !policy.admits(binding.role, PlatformPermission::TenantSupportManage)
        {
            return Err("tenant support approver has no current management authority".into());
        }
        let mut value = Self {
            grant_id: proposal.id,
            contract_digest: proposal.contract.digest().clone(),
            approver_id,
            authentication,
            policy_revision_id: policy.id,
            policy_digest: policy.contract.digest().clone(),
            binding_id: binding.id,
            binding_version: binding.aggregate_version,
            approved_at: canonical_timestamp(approved_at),
            digest: Sha256Digest::parse(format!("sha256:{}", "0".repeat(64)))?,
        };
        value.digest = value.compute_digest()?;
        value.validate_against(proposal)?;
        Ok(value)
    }

    pub fn validate_against(&self, proposal: &TenantSupportGrantProposal) -> Result<(), String> {
        proposal.validate()?;
        self.authentication.validate()?;
        Sha256Digest::parse(self.contract_digest.as_str())?;
        Sha256Digest::parse(self.policy_digest.as_str())?;
        Sha256Digest::parse(self.digest.as_str())?;
        if self.grant_id.as_uuid().is_nil()
            || self.grant_id != proposal.id
            || self.contract_digest != *proposal.contract.digest()
            || self.approver_id.as_uuid().is_nil()
            || !proposal
                .contract
                .spec()
                .approver_ids
                .contains(&self.approver_id)
            || self.approver_id == proposal.requested_by
            || self.policy_revision_id.as_uuid().is_nil()
            || self.binding_id.as_uuid().is_nil()
            || self.binding_version == 0
            || self.approved_at != canonical_timestamp(self.approved_at)
            || self.approved_at < proposal.requested_at
            || self.approved_at >= proposal.contract.spec().expires_at
            || self.compute_digest()? != self.digest
        {
            return Err("tenant support grant approval evidence is invalid".into());
        }
        Ok(())
    }

    fn compute_digest(&self) -> Result<Sha256Digest, String> {
        let content = TenantSupportGrantApprovalDigestContent {
            grant_id: self.grant_id,
            contract_digest: &self.contract_digest,
            approver_id: self.approver_id,
            authentication: &self.authentication,
            policy_revision_id: self.policy_revision_id,
            policy_digest: &self.policy_digest,
            binding_id: self.binding_id,
            binding_version: self.binding_version,
            approved_at: self.approved_at,
        };
        let canonical = canonical_json_bounded(
            &content,
            32 * 1024,
            "tenant support grant approval digest content",
        )?;
        Sha256Digest::parse(sha256_digest(&canonical))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantSupportGrantApprovalOutcome {
    pub proposal: TenantSupportGrantProposal,
    pub approval: TenantSupportGrantApproval,
    pub grant: Option<TenantSupportGrant>,
}

impl TenantSupportGrantApprovalOutcome {
    pub fn validate(&self) -> Result<(), String> {
        self.proposal.validate()?;
        self.approval.validate_against(&self.proposal)?;
        if let Some(grant) = &self.grant {
            grant.validate()?;
            if grant.id != self.proposal.id || grant.contract != self.proposal.contract {
                return Err("accepted tenant support grant does not match its proposal".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::AcceptedPlatformRolePolicyRevision;
    use crate::modules::identity::domain::value_objects::{
        PlatformRole, PlatformRolePolicyContract, TenantNotificationRequirement,
        TenantSupportApprovalRequirement, TenantSupportGrantContractSpec, TenantSupportGrantMode,
        TenantSupportPermission,
    };
    use crate::modules::shared_kernel::domain::{
        InstallationId, OrganizationId, PlatformRolePolicyId, ScopeContext,
    };
    use chrono::{Duration, TimeZone};

    fn timestamp() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 29, 12, 0, 0)
            .single()
            .expect("timestamp")
    }

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }

    fn authentication(byte: char) -> DecisionEvidenceRef {
        DecisionEvidenceRef::new(
            format!("urn:a3s:cloud:identity:authentication:{byte}"),
            digest(byte),
        )
        .expect("authentication")
    }

    #[test]
    fn approval_binds_actual_actor_policy_and_binding_evidence() {
        let installation_id = InstallationId::new();
        let subject = PrincipalId::new();
        let requester = PrincipalId::new();
        let approver = PrincipalId::new();
        let contract = TenantSupportGrantContract::from_spec(TenantSupportGrantContractSpec {
            grant_id: TenantSupportGrantId::new(),
            principal_id: subject,
            scope: ScopeContext::organization(installation_id, OrganizationId::new())
                .expect("scope"),
            permissions: vec![TenantSupportPermission::HealthRead],
            case_reference: "INC-APPROVAL-1".into(),
            justification_digest: digest('a'),
            mode: TenantSupportGrantMode::Standard,
            approval_requirement: TenantSupportApprovalRequirement::Single,
            approver_ids: vec![approver],
            tenant_notification: TenantNotificationRequirement::Required,
            security_alert_required: false,
            post_incident_review_required: false,
            starts_at: timestamp(),
            expires_at: timestamp() + Duration::hours(1),
        })
        .expect("contract");
        let proposal = TenantSupportGrantProposal::propose(
            contract,
            requester,
            authentication('b'),
            timestamp() - Duration::minutes(1),
        )
        .expect("proposal");
        let policy = AcceptedPlatformRolePolicyRevision::accept(
            PlatformRolePolicyContract::baseline(installation_id, PlatformRolePolicyId::new())
                .expect("policy contract"),
            1,
            requester,
            timestamp() - Duration::minutes(2),
        )
        .expect("policy");
        let binding = PlatformRoleBinding::create(
            PlatformRoleBindingId::new(),
            installation_id,
            approver,
            PlatformRole::PlatformAdmin,
            &policy,
            requester,
            timestamp() - Duration::minutes(2),
        )
        .expect("binding");
        let approval = TenantSupportGrantApproval::record(
            &proposal,
            approver,
            authentication('c'),
            &policy,
            &binding,
            timestamp(),
        )
        .expect("approval");
        approval.validate_against(&proposal).expect("valid");

        let mut forged = approval;
        forged.binding_version += 1;
        assert!(forged.validate_against(&proposal).is_err());
    }

    #[test]
    fn requester_cannot_be_declared_as_its_own_approver() {
        let installation_id = InstallationId::new();
        let requester = PrincipalId::new();
        let contract = TenantSupportGrantContract::from_spec(TenantSupportGrantContractSpec {
            grant_id: TenantSupportGrantId::new(),
            principal_id: PrincipalId::new(),
            scope: ScopeContext::organization(installation_id, OrganizationId::new())
                .expect("scope"),
            permissions: vec![TenantSupportPermission::HealthRead],
            case_reference: "INC-APPROVAL-2".into(),
            justification_digest: digest('d'),
            mode: TenantSupportGrantMode::Standard,
            approval_requirement: TenantSupportApprovalRequirement::Single,
            approver_ids: vec![requester],
            tenant_notification: TenantNotificationRequirement::Required,
            security_alert_required: false,
            post_incident_review_required: false,
            starts_at: timestamp(),
            expires_at: timestamp() + Duration::hours(1),
        })
        .expect("contract");
        assert!(TenantSupportGrantProposal::propose(
            contract,
            requester,
            authentication('e'),
            timestamp()
        )
        .is_err());
    }
}
