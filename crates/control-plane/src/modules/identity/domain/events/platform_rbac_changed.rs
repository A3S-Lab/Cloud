use crate::modules::identity::domain::entities::{
    AcceptedPlatformRolePolicyRevision, PlatformRoleBinding,
};
use crate::modules::identity::domain::value_objects::PlatformRole;
use a3s_cloud_contracts::{CloudScopeRef, DomainEventEnvelope};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformRolePolicyAccepted {
    pub installation_id: Uuid,
    pub policy_id: Uuid,
    pub revision_id: Uuid,
    pub revision_number: u64,
    pub digest: String,
    pub accepted_by: Uuid,
}

impl PlatformRolePolicyAccepted {
    pub fn envelope(
        revision: &AcceptedPlatformRolePolicyRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            installation_id: revision.installation_id.as_uuid(),
            policy_id: revision.policy_id.as_uuid(),
            revision_id: revision.id.as_uuid(),
            revision_number: revision.revision_number,
            digest: revision.contract.digest().as_str().to_owned(),
            accepted_by: revision.accepted_by.as_uuid(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "identity.platform-role-policy.accepted".into(),
            schema_version: 1,
            scope: CloudScopeRef::Installation {
                installation_id: revision.installation_id.as_uuid(),
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformRoleBindingChanged {
    pub installation_id: Uuid,
    pub binding_id: Uuid,
    pub principal_id: Uuid,
    pub role: PlatformRole,
    pub previous_role: Option<PlatformRole>,
    pub updated_by: Uuid,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl PlatformRoleBindingChanged {
    pub fn created(
        binding: &PlatformRoleBinding,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "identity.platform-role-binding.created",
            binding,
            None,
            correlation_id,
        )
    }

    pub fn role_changed(
        binding: &PlatformRoleBinding,
        previous_role: PlatformRole,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "identity.platform-role-binding.role-changed",
            binding,
            Some(previous_role),
            correlation_id,
        )
    }

    pub fn revoked(
        binding: &PlatformRoleBinding,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "identity.platform-role-binding.revoked",
            binding,
            None,
            correlation_id,
        )
    }

    fn envelope(
        event_key: &str,
        binding: &PlatformRoleBinding,
        previous_role: Option<PlatformRole>,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            installation_id: binding.installation_id.as_uuid(),
            binding_id: binding.id.as_uuid(),
            principal_id: binding.principal_id.as_uuid(),
            role: binding.role,
            previous_role,
            updated_by: binding.updated_by.as_uuid(),
            revoked_at: binding.revoked_at,
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            scope: CloudScopeRef::Installation {
                installation_id: binding.installation_id.as_uuid(),
            },
            aggregate_id: binding.id.as_uuid(),
            aggregate_version: binding.aggregate_version,
            occurred_at: binding.updated_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
