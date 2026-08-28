use crate::modules::plugins::domain::entities::PluginRegistry;
use crate::modules::shared_kernel::domain::{OrganizationId, PluginRegistryId, PrincipalId};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRegistryEnrolled {
    pub organization_id: OrganizationId,
    pub registry_id: PluginRegistryId,
    pub name: String,
    pub endpoint: String,
    pub root_object_ref: String,
    pub root_sha256: String,
    pub root_version: u64,
    pub actor_id: PrincipalId,
}

impl PluginRegistryEnrolled {
    pub fn envelope(registry: &PluginRegistry) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "plugins.registry.enrolled".into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: registry.organization_id.as_uuid(),
            },
            aggregate_id: registry.id.as_uuid(),
            aggregate_version: registry.aggregate_version,
            occurred_at: registry.created_at,
            correlation_id: registry.last_request_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: registry.organization_id,
                registry_id: registry.id,
                name: registry.name.as_str().to_owned(),
                endpoint: registry.endpoint.as_str().to_owned(),
                root_object_ref: registry.trust_root.object_ref().as_str().to_owned(),
                root_sha256: registry.trust_root.digest().as_str().to_owned(),
                root_version: registry.trust_root.version(),
                actor_id: registry.last_actor_id,
            })?,
        })
    }
}
