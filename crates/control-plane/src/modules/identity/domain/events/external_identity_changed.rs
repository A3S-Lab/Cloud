use crate::modules::identity::domain::entities::ExternalIdentityLink;
use crate::modules::shared_kernel::domain::{ExternalIdentityLinkId, OrganizationId, PrincipalId};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalIdentityChanged {
    pub link_id: ExternalIdentityLinkId,
    pub principal_id: PrincipalId,
    pub provider_key: String,
    pub issuer: String,
}

impl ExternalIdentityChanged {
    pub fn linked(
        link: &ExternalIdentityLink,
        organization_id: OrganizationId,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "identity.external-identity.linked",
            link,
            organization_id,
            correlation_id,
        )
    }

    pub fn verified(
        link: &ExternalIdentityLink,
        organization_id: OrganizationId,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "identity.external-identity.verified",
            link,
            organization_id,
            correlation_id,
        )
    }

    fn envelope(
        event_key: &str,
        link: &ExternalIdentityLink,
        organization_id: OrganizationId,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            link_id: link.id,
            principal_id: link.principal_id,
            provider_key: link.provider_key.as_str().to_owned(),
            issuer: link.issuer.as_str().to_owned(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: organization_id.as_uuid(),
            },
            aggregate_id: link.id.as_uuid(),
            aggregate_version: link.aggregate_version,
            occurred_at: link.last_verified_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
