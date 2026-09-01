use crate::modules::plugins::domain::entities::PluginRegistry;
use crate::modules::plugins::domain::repositories::{
    CreatePluginRegistryWrite, IPluginRegistryRepository,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OrganizationId, PluginRegistryId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryPluginRegistryRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    registries: BTreeMap<(OrganizationId, PluginRegistryId), PluginRegistry>,
    names: BTreeMap<(OrganizationId, String), PluginRegistryId>,
    endpoints: BTreeMap<(OrganizationId, String), PluginRegistryId>,
    idempotency: BTreeMap<(String, String), (String, PluginRegistry)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryPluginRegistryRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IPluginRegistryRepository for InMemoryPluginRegistryRepository {
    async fn create(
        &self,
        write: CreatePluginRegistryWrite,
    ) -> Result<IdempotentWrite<PluginRegistry>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let CreatePluginRegistryWrite {
            registry,
            event,
            idempotency,
            ..
        } = write;
        let mut state = self.state.write().await;
        let key = (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, existing)) = state.idempotency.get(&key) {
            if digest != &idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            CreatePluginRegistryWrite::validate_replay(&registry, existing)?;
            return Ok(IdempotentWrite {
                value: existing.clone(),
                replayed: true,
            });
        }
        let name_key = (registry.organization_id, registry.name.key().to_owned());
        if state.names.contains_key(&name_key) {
            return Err(RepositoryError::Conflict(
                "plugin registry name is already in use".into(),
            ));
        }
        let endpoint_key = (
            registry.organization_id,
            registry.endpoint.as_str().to_owned(),
        );
        if state.endpoints.contains_key(&endpoint_key) {
            return Err(RepositoryError::Conflict(
                "plugin registry endpoint is already enrolled".into(),
            ));
        }
        state.names.insert(name_key, registry.id);
        state.endpoints.insert(endpoint_key, registry.id);
        state
            .registries
            .insert((registry.organization_id, registry.id), registry.clone());
        state
            .idempotency
            .insert(key, (idempotency.request_digest, registry.clone()));
        state.outbox.push(event);
        Ok(IdempotentWrite {
            value: registry,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        registry_id: PluginRegistryId,
    ) -> Result<Option<PluginRegistry>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .registries
            .get(&(organization_id, registry_id))
            .cloned())
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<PluginRegistry>, RepositoryError> {
        let mut registries = self
            .state
            .read()
            .await
            .registries
            .values()
            .filter(|registry| registry.organization_id == organization_id)
            .cloned()
            .collect::<Vec<_>>();
        registries.sort_by_key(|registry| (registry.created_at, registry.id));
        Ok(registries)
    }
}

#[cfg(test)]
mod tests {
    use super::InMemoryPluginRegistryRepository;
    use crate::modules::plugins::domain::entities::{NewPluginRegistry, PluginRegistry};
    use crate::modules::plugins::domain::events::PluginRegistryEnrolled;
    use crate::modules::plugins::domain::repositories::{
        CreatePluginRegistryWrite, IPluginRegistryRepository,
    };
    use crate::modules::plugins::domain::services::PluginRegistryEnrollmentAuthorization;
    use crate::modules::plugins::domain::value_objects::{PluginRegistryEndpoint, PluginTrustRoot};
    use crate::modules::shared_kernel::domain::{
        OrganizationId, PluginRegistryId, PrincipalId, RepositoryError, ResourceName, Sha256Digest,
    };
    use chrono::Utc;
    use uuid::Uuid;

    fn registry(
        organization_id: OrganizationId,
        actor_id: PrincipalId,
        request_id: Uuid,
    ) -> PluginRegistry {
        PluginRegistry::enroll(NewPluginRegistry {
            organization_id,
            id: PluginRegistryId::new(),
            name: ResourceName::parse("Official").expect("name"),
            endpoint: PluginRegistryEndpoint::parse("https://registry.example/a3s")
                .expect("endpoint"),
            trust_root: PluginTrustRoot::from_digest(
                Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
                7,
            )
            .expect("trust root"),
            actor_id,
            request_id,
            enrolled_at: Utc::now(),
        })
        .expect("registry")
    }

    fn write(registry: PluginRegistry, key: &str) -> CreatePluginRegistryWrite {
        let authorization = PluginRegistryEnrollmentAuthorization::new(
            registry.organization_id,
            registry.last_actor_id,
        )
        .expect("authorization");
        let event = PluginRegistryEnrolled::envelope(&registry).expect("event");
        let idempotency =
            CreatePluginRegistryWrite::idempotency_for(&registry, key).expect("idempotency");
        CreatePluginRegistryWrite {
            idempotency,
            registry,
            event,
            authorization,
        }
    }

    #[tokio::test]
    async fn create_replays_once_and_remains_tenant_scoped() {
        let repository = InMemoryPluginRegistryRepository::new();
        let organization_id = OrganizationId::new();
        let actor_id = PrincipalId::new();
        let enrolled_registry = registry(organization_id, actor_id, Uuid::now_v7());
        let replay_request = registry(organization_id, actor_id, Uuid::now_v7());

        let created = repository
            .create(write(enrolled_registry.clone(), "enroll-1"))
            .await
            .expect("create");
        let replayed = repository
            .create(write(replay_request, "enroll-1"))
            .await
            .expect("replay");

        assert!(!created.replayed);
        assert!(replayed.replayed);
        assert_eq!(replayed.value, enrolled_registry);
        assert_eq!(repository.outbox_events().await.len(), 1);
        assert_eq!(
            repository.list(organization_id).await.expect("list").len(),
            1
        );
        assert!(repository
            .find(OrganizationId::new(), enrolled_registry.id)
            .await
            .expect("foreign lookup")
            .is_none());
    }

    #[tokio::test]
    async fn tampered_event_payload_fails_without_writing_state() {
        let repository = InMemoryPluginRegistryRepository::new();
        let organization_id = OrganizationId::new();
        let registry = registry(organization_id, PrincipalId::new(), Uuid::now_v7());
        let mut write = write(registry, "tampered");
        write.event.payload["root_version"] = serde_json::json!(8);

        let error = repository.create(write).await.expect_err("tampered event");

        assert!(matches!(error, RepositoryError::Storage(_)));
        assert!(repository.outbox_events().await.is_empty());
        assert!(repository
            .list(organization_id)
            .await
            .expect("list")
            .is_empty());
    }

    #[tokio::test]
    async fn mismatched_enrollment_authorization_fails_without_writing_state() {
        let repository = InMemoryPluginRegistryRepository::new();
        let organization_id = OrganizationId::new();
        let registry = registry(organization_id, PrincipalId::new(), Uuid::now_v7());
        let mut write = write(registry, "wrong-authorization");
        write.authorization = PluginRegistryEnrollmentAuthorization::new(
            OrganizationId::new(),
            write.authorization.actor_id(),
        )
        .expect("foreign authorization");

        let error = repository
            .create(write)
            .await
            .expect_err("authorization mismatch");

        assert!(matches!(error, RepositoryError::Storage(_)));
        assert!(repository
            .list(organization_id)
            .await
            .expect("registry list")
            .is_empty());
    }

    #[tokio::test]
    async fn idempotency_replay_is_bound_to_the_authorized_actor() {
        let repository = InMemoryPluginRegistryRepository::new();
        let organization_id = OrganizationId::new();
        repository
            .create(write(
                registry(organization_id, PrincipalId::new(), Uuid::now_v7()),
                "actor-bound",
            ))
            .await
            .expect("first actor enrollment");

        let error = repository
            .create(write(
                registry(organization_id, PrincipalId::new(), Uuid::now_v7()),
                "actor-bound",
            ))
            .await
            .expect_err("different actor replay");

        assert_eq!(error, RepositoryError::IdempotencyConflict);
        assert_eq!(repository.outbox_events().await.len(), 1);
    }

    #[tokio::test]
    async fn idempotency_keys_are_tenant_scoped_and_reject_changed_input() {
        let repository = InMemoryPluginRegistryRepository::new();
        let first_organization = OrganizationId::new();
        let first = registry(first_organization, PrincipalId::new(), Uuid::now_v7());
        repository
            .create(write(first.clone(), "shared-key"))
            .await
            .expect("first enrollment");

        let second_organization = OrganizationId::new();
        let second = registry(second_organization, PrincipalId::new(), Uuid::now_v7());
        repository
            .create(write(second, "shared-key"))
            .await
            .expect("second tenant enrollment");

        let mut changed = first;
        changed.id = PluginRegistryId::new();
        changed.name = ResourceName::parse("Mirror").expect("name");
        changed.endpoint =
            PluginRegistryEndpoint::parse("https://mirror.example/a3s").expect("endpoint");
        changed.last_request_id = Uuid::now_v7();
        let error = repository
            .create(write(changed, "shared-key"))
            .await
            .expect_err("changed input");

        assert_eq!(error, RepositoryError::IdempotencyConflict);
        assert_eq!(repository.outbox_events().await.len(), 2);
    }
}
