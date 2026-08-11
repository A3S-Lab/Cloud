use crate::modules::plugins::domain::value_objects::{
    PluginRegistryEndpoint, PluginRegistryState, PluginTrustRoot,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, PluginRegistryId, PrincipalId, ResourceName,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPluginRegistry {
    pub organization_id: OrganizationId,
    pub id: PluginRegistryId,
    pub name: ResourceName,
    pub endpoint: PluginRegistryEndpoint,
    pub trust_root: PluginTrustRoot,
    pub actor_id: PrincipalId,
    pub request_id: Uuid,
    pub enrolled_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRegistry {
    pub organization_id: OrganizationId,
    pub id: PluginRegistryId,
    pub name: ResourceName,
    pub endpoint: PluginRegistryEndpoint,
    pub trust_root: PluginTrustRoot,
    pub state: PluginRegistryState,
    pub aggregate_version: u64,
    pub last_actor_id: PrincipalId,
    pub last_request_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PluginRegistry {
    pub fn enroll(input: NewPluginRegistry) -> Result<Self, String> {
        let enrolled_at = canonical_timestamp(input.enrolled_at);
        let registry = Self {
            organization_id: input.organization_id,
            id: input.id,
            name: input.name,
            endpoint: input.endpoint,
            trust_root: input.trust_root,
            state: PluginRegistryState::Active,
            aggregate_version: 1,
            last_actor_id: input.actor_id,
            last_request_id: input.request_id,
            created_at: enrolled_at,
            updated_at: enrolled_at,
        };
        registry.validate()?;
        Ok(registry)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.last_actor_id.as_uuid().is_nil()
            || self.last_request_id.is_nil()
            || self.aggregate_version == 0
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
            || ResourceName::parse(self.name.as_str())? != self.name
            || PluginRegistryEndpoint::parse(self.endpoint.as_str())? != self.endpoint
        {
            return Err("plugin registry identity, version, or timestamps are invalid".into());
        }
        self.trust_root.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{NewPluginRegistry, PluginRegistry};
    use crate::modules::plugins::domain::value_objects::{
        PluginRegistryEndpoint, PluginRegistryState, PluginTrustRoot,
    };
    use crate::modules::shared_kernel::domain::{
        OrganizationId, PluginRegistryId, PrincipalId, ResourceName, Sha256Digest,
    };
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    #[test]
    fn enrollment_starts_one_active_registry_with_exact_root_evidence() {
        let root = PluginTrustRoot::from_digest(
            Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
            7,
        )
        .expect("root");
        let registry = PluginRegistry::enroll(NewPluginRegistry {
            organization_id: OrganizationId::new(),
            id: PluginRegistryId::new(),
            name: ResourceName::parse("Official").expect("name"),
            endpoint: PluginRegistryEndpoint::parse("https://registry.example/a3s")
                .expect("endpoint"),
            trust_root: root,
            actor_id: PrincipalId::new(),
            request_id: Uuid::now_v7(),
            enrolled_at: Utc
                .with_ymd_and_hms(2026, 8, 11, 4, 5, 6)
                .single()
                .expect("timestamp"),
        })
        .expect("registry");

        assert_eq!(registry.state, PluginRegistryState::Active);
        assert_eq!(registry.aggregate_version, 1);
        assert_eq!(registry.trust_root.version(), 7);
        assert_eq!(registry.created_at, registry.updated_at);
    }
}
