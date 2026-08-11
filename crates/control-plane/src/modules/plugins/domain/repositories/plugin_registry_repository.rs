use crate::modules::plugins::domain::entities::PluginRegistry;
use crate::modules::plugins::domain::events::PluginRegistryEnrolled;
use crate::modules::plugins::domain::value_objects::PluginRegistryState;
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, PluginRegistryId, PrincipalId,
    RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreatePluginRegistryWrite {
    pub registry: PluginRegistry,
    pub event: DomainEventEnvelope,
    pub actor_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl CreatePluginRegistryWrite {
    pub fn idempotency_for(
        registry: &PluginRegistry,
        key: impl Into<String>,
    ) -> Result<IdempotencyRequest, String> {
        registry.validate()?;
        let canonical_request = serde_json::to_vec(&serde_json::json!({
            "organizationId": registry.organization_id,
            "name": registry.name.as_str(),
            "endpoint": registry.endpoint.as_str(),
            "rootObjectRef": registry.trust_root.object_ref().as_str(),
            "rootSha256": registry.trust_root.digest().as_str(),
            "rootVersion": registry.trust_root.version(),
        }))
        .map_err(|error| format!("serialize plugin registry enrollment: {error}"))?;
        IdempotencyRequest::new(
            format!(
                "organizations/{}/plugin-registries",
                registry.organization_id
            ),
            key,
            &canonical_request,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        self.idempotency.validate()?;
        let registry = &self.registry;
        let event = &self.event;
        let expected_idempotency = Self::idempotency_for(registry, self.idempotency.key.clone())?;
        if registry.state != PluginRegistryState::Active
            || registry.aggregate_version != 1
            || registry.created_at != registry.updated_at
            || registry.last_actor_id != self.actor_id
            || registry.last_request_id != self.request_id
            || event.event_id.is_nil()
            || event.event_key != "plugins.registry.enrolled"
            || event.schema_version != 1
            || event.organization_id != registry.organization_id.as_uuid()
            || event.aggregate_id != registry.id.as_uuid()
            || event.aggregate_version != registry.aggregate_version
            || event.occurred_at != registry.created_at
            || event.correlation_id != self.request_id
            || event.causation_id.is_some_and(|id| id.is_nil())
            || self.idempotency != expected_idempotency
        {
            return Err("plugin registry write evidence is inconsistent".into());
        }
        let payload: PluginRegistryEnrolled = serde_json::from_value(event.payload.clone())
            .map_err(|error| format!("plugin registry event payload is invalid: {error}"))?;
        if payload.organization_id != registry.organization_id
            || payload.registry_id != registry.id
            || payload.name != registry.name.as_str()
            || payload.endpoint != registry.endpoint.as_str()
            || payload.root_object_ref != registry.trust_root.object_ref().as_str()
            || payload.root_sha256 != registry.trust_root.digest().as_str()
            || payload.root_version != registry.trust_root.version()
            || payload.actor_id != registry.last_actor_id
        {
            return Err("plugin registry event payload is inconsistent".into());
        }
        Ok(())
    }

    pub(crate) fn validate_replay(
        requested: &PluginRegistry,
        replayed: &PluginRegistry,
    ) -> Result<(), String> {
        replayed.validate()?;
        if replayed.organization_id != requested.organization_id
            || replayed.name != requested.name
            || replayed.endpoint != requested.endpoint
            || replayed.trust_root != requested.trust_root
            || replayed.state != PluginRegistryState::Active
            || replayed.aggregate_version != 1
            || replayed.created_at != replayed.updated_at
        {
            return Err("plugin registry idempotency replay is inconsistent".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait IPluginRegistryRepository: Send + Sync {
    async fn create(
        &self,
        write: CreatePluginRegistryWrite,
    ) -> Result<IdempotentWrite<PluginRegistry>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        registry_id: PluginRegistryId,
    ) -> Result<Option<PluginRegistry>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<PluginRegistry>, RepositoryError>;
}
