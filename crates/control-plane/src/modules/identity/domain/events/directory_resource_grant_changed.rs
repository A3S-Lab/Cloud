use crate::modules::identity::domain::entities::DirectoryResourceGrant;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryResourceGrantChanged {
    pub resource_grant_id: Uuid,
    pub subject_kind: String,
    pub issuer: String,
    pub subject_id: Uuid,
    pub scope_kind: String,
    pub project_id: Option<Uuid>,
    pub environment_id: Option<Uuid>,
    pub node_id: Option<Uuid>,
}

impl DirectoryResourceGrantChanged {
    pub fn created(
        grant: &DirectoryResourceGrant,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "identity.directory-resource-grant.created",
            grant,
            correlation_id,
        )
    }

    pub fn revoked(
        grant: &DirectoryResourceGrant,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "identity.directory-resource-grant.revoked",
            grant,
            correlation_id,
        )
    }

    fn envelope(
        event_key: &str,
        grant: &DirectoryResourceGrant,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            resource_grant_id: grant.id.as_uuid(),
            subject_kind: grant.subject.kind().as_str().to_owned(),
            issuer: grant.subject.issuer().as_str().to_owned(),
            subject_id: grant.subject.subject_id(),
            scope_kind: grant.scope.kind().to_owned(),
            project_id: grant.scope.project_id().map(|id| id.as_uuid()),
            environment_id: grant.scope.environment_id().map(|id| id.as_uuid()),
            node_id: grant.scope.node_id().map(|id| id.as_uuid()),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: grant.organization_id.as_uuid(),
            },
            aggregate_id: grant.id.as_uuid(),
            aggregate_version: grant.aggregate_version,
            occurred_at: grant.updated_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
