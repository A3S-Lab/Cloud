use crate::modules::identity::domain::entities::{
    AcceptedTrustDomainRevision, AcceptedWorkloadIdentityPolicyRevision,
};
use a3s_cloud_contracts::{CloudScopeRef, DomainEventEnvelope};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustDomainRevisionAccepted {
    pub installation_id: Uuid,
    pub trust_domain_id: Uuid,
    pub revision_id: Uuid,
    pub revision_number: u64,
    pub name: String,
    pub digest: String,
    pub accepted_by: Uuid,
}

impl TrustDomainRevisionAccepted {
    pub fn envelope(
        revision: &AcceptedTrustDomainRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            installation_id: revision.installation_id.as_uuid(),
            trust_domain_id: revision.trust_domain_id.as_uuid(),
            revision_id: revision.id.as_uuid(),
            revision_number: revision.revision_number,
            name: revision.contract.spec().name.as_str().to_owned(),
            digest: revision.contract.digest().as_str().to_owned(),
            accepted_by: revision.accepted_by.as_uuid(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "identity.trust-domain.revision-accepted".into(),
            schema_version: 1,
            scope: CloudScopeRef::Installation {
                installation_id: revision.installation_id.as_uuid(),
            },
            aggregate_id: revision.trust_domain_id.as_uuid(),
            aggregate_version: revision.revision_number,
            occurred_at: revision.accepted_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadIdentityPolicyRevisionAccepted {
    pub installation_id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub trust_domain_id: Uuid,
    pub trust_domain_revision_id: Uuid,
    pub policy_id: Uuid,
    pub revision_id: Uuid,
    pub revision_number: u64,
    pub workload_id: Uuid,
    pub workload_revision_id: Uuid,
    pub digest: String,
    pub accepted_by: Uuid,
}

impl WorkloadIdentityPolicyRevisionAccepted {
    pub fn envelope(
        revision: &AcceptedWorkloadIdentityPolicyRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let spec = revision.contract.spec();
        let payload = Self {
            installation_id: revision.installation_id.as_uuid(),
            organization_id: spec.organization_id.as_uuid(),
            project_id: spec.project_id.as_uuid(),
            environment_id: spec.environment_id.as_uuid(),
            trust_domain_id: spec.trust_domain_id.as_uuid(),
            trust_domain_revision_id: spec.trust_domain_revision_id.as_uuid(),
            policy_id: revision.policy_id.as_uuid(),
            revision_id: revision.id.as_uuid(),
            revision_number: revision.revision_number,
            workload_id: spec.workload_id.as_uuid(),
            workload_revision_id: spec.workload_revision_id.as_uuid(),
            digest: revision.contract.digest().as_str().to_owned(),
            accepted_by: revision.accepted_by.as_uuid(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "identity.workload-identity-policy.revision-accepted".into(),
            schema_version: 1,
            scope: CloudScopeRef::Environment {
                organization_id: spec.organization_id.as_uuid(),
                project_id: spec.project_id.as_uuid(),
                environment_id: spec.environment_id.as_uuid(),
            },
            aggregate_id: revision.policy_id.as_uuid(),
            aggregate_version: revision.revision_number,
            occurred_at: revision.accepted_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
