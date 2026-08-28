use crate::modules::identity::domain::entities::MembershipInvitation;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipInvitationChanged {
    pub invitation_id: Uuid,
    pub principal_id: Uuid,
    pub role: String,
    pub accepted_membership_id: Option<Uuid>,
}

impl MembershipInvitationChanged {
    pub fn created(
        invitation: &MembershipInvitation,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "identity.membership-invitation.created",
            invitation,
            correlation_id,
        )
    }

    pub fn accepted(
        invitation: &MembershipInvitation,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "identity.membership-invitation.accepted",
            invitation,
            correlation_id,
        )
    }

    pub fn revoked(
        invitation: &MembershipInvitation,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "identity.membership-invitation.revoked",
            invitation,
            correlation_id,
        )
    }

    fn envelope(
        event_key: &str,
        invitation: &MembershipInvitation,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            invitation_id: invitation.id.as_uuid(),
            principal_id: invitation.principal_id.as_uuid(),
            role: invitation.role.as_str().to_owned(),
            accepted_membership_id: invitation.accepted_membership_id.map(|id| id.as_uuid()),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: invitation.organization_id.as_uuid(),
            },
            aggregate_id: invitation.id.as_uuid(),
            aggregate_version: invitation.aggregate_version,
            occurred_at: invitation.updated_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
