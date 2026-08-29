use crate::modules::identity::domain::entities::{
    TenantSupportGrant, TenantSupportGrantApproval, TenantSupportGrantProposal,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantSupportGrantProposed {
    pub grant_id: Uuid,
    pub principal_id: Uuid,
    pub contract_digest: String,
    pub requested_by: Uuid,
    pub required_approvers: Vec<Uuid>,
}

impl TenantSupportGrantProposed {
    pub fn envelope(
        proposal: &TenantSupportGrantProposal,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            grant_id: proposal.id.as_uuid(),
            principal_id: proposal.contract.spec().principal_id.as_uuid(),
            contract_digest: proposal.contract.digest().as_str().to_owned(),
            requested_by: proposal.requested_by.as_uuid(),
            required_approvers: proposal
                .contract
                .spec()
                .approver_ids
                .iter()
                .map(|id| id.as_uuid())
                .collect(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "identity.tenant-support-grant.proposed".into(),
            schema_version: 1,
            scope: proposal.contract.spec().scope.reference(),
            aggregate_id: proposal.id.as_uuid(),
            aggregate_version: 1,
            occurred_at: proposal.requested_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantSupportGrantApproved {
    pub grant_id: Uuid,
    pub approver_id: Uuid,
    pub contract_digest: String,
    pub policy_revision_id: Uuid,
    pub binding_id: Uuid,
    pub binding_version: u64,
    pub evidence_digest: String,
}

impl TenantSupportGrantApproved {
    pub fn envelope(
        proposal: &TenantSupportGrantProposal,
        approval: &TenantSupportGrantApproval,
        approval_ordinal: u64,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            grant_id: approval.grant_id.as_uuid(),
            approver_id: approval.approver_id.as_uuid(),
            contract_digest: approval.contract_digest.as_str().to_owned(),
            policy_revision_id: approval.policy_revision_id.as_uuid(),
            binding_id: approval.binding_id.as_uuid(),
            binding_version: approval.binding_version,
            evidence_digest: approval.digest.as_str().to_owned(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "identity.tenant-support-grant.approved".into(),
            schema_version: 1,
            scope: proposal.contract.spec().scope.reference(),
            aggregate_id: proposal.id.as_uuid(),
            aggregate_version: approval_ordinal,
            occurred_at: approval.approved_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantSupportGrantChanged {
    pub grant_id: Uuid,
    pub principal_id: Uuid,
    pub contract_digest: String,
    pub aggregate_version: u64,
    pub revocation_generation: u64,
    pub revoked_by: Option<Uuid>,
}

impl TenantSupportGrantChanged {
    pub fn accepted(
        grant: &TenantSupportGrant,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "identity.tenant-support-grant.accepted",
            grant,
            correlation_id,
        )
    }

    pub fn revoked(
        grant: &TenantSupportGrant,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "identity.tenant-support-grant.revoked",
            grant,
            correlation_id,
        )
    }

    fn envelope(
        event_key: &str,
        grant: &TenantSupportGrant,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            grant_id: grant.id.as_uuid(),
            principal_id: grant.contract.spec().principal_id.as_uuid(),
            contract_digest: grant.contract.digest().as_str().to_owned(),
            aggregate_version: grant.aggregate_version,
            revocation_generation: grant.revocation_generation,
            revoked_by: grant.revoked_by.map(|id| id.as_uuid()),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            scope: grant.contract.spec().scope.reference(),
            aggregate_id: grant.id.as_uuid(),
            aggregate_version: grant.aggregate_version,
            occurred_at: grant.revoked_at.unwrap_or(grant.accepted_at),
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
