use super::schema::DigestBoundJsonSchemaValidator;
use crate::modules::automations::domain::{
    IAutomationWebhookSchemaRegistry, IAutomationWebhookSchemaValidator,
};
use crate::modules::shared_kernel::domain::{canonical_json_bounded, sha256_digest, Sha256Digest};
use a3s_cloud_contracts::{AutomationWebhookEndpointV1, AutomationWebhookRequestV1};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Component adapter that selects the exact endpoint-bound schema and then
/// delegates evaluation to the digest-bound validator. It never caches a
/// mutable latest document or follows an external reference.
pub struct RegistryBackedAutomationWebhookSchemaValidator {
    registry: Arc<dyn IAutomationWebhookSchemaRegistry>,
}

impl RegistryBackedAutomationWebhookSchemaValidator {
    pub fn new(registry: Arc<dyn IAutomationWebhookSchemaRegistry>) -> Self {
        Self { registry }
    }
}

impl std::fmt::Debug for RegistryBackedAutomationWebhookSchemaValidator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RegistryBackedAutomationWebhookSchemaValidator")
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl IAutomationWebhookSchemaValidator for RegistryBackedAutomationWebhookSchemaValidator {
    async fn validate(
        &self,
        endpoint: &AutomationWebhookEndpointV1,
        request: &AutomationWebhookRequestV1,
    ) -> Result<(), String> {
        endpoint.validate().map_err(|_| {
            "Automation webhook endpoint is invalid for schema selection".to_owned()
        })?;
        let digest = Sha256Digest::parse(endpoint.request_schema_digest.clone())
            .map_err(|_| "Automation webhook endpoint schema digest is invalid".to_owned())?;
        let Some(schema) = self
            .registry
            .resolve(&digest)
            .await
            .map_err(|_| "Automation webhook schema selection failed".to_owned())?
        else {
            return Err("Automation webhook request schema is unavailable".into());
        };
        let validator = DigestBoundJsonSchemaValidator::new(digest.as_str(), schema)
            .map_err(|_| "Automation webhook selected request schema is invalid".to_owned())?;
        validator.validate(endpoint, request).await
    }
}

/// Small deterministic registry adapter for local composition and tests. A
/// production owner may implement the same port over its immutable registry;
/// Automations only receives exact digest lookups.
#[derive(Default)]
pub struct InMemoryAutomationWebhookSchemaRegistry {
    schemas: RwLock<BTreeMap<Sha256Digest, Value>>,
}

impl InMemoryAutomationWebhookSchemaRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(&self, schema: Value) -> Result<Sha256Digest, String> {
        let canonical = canonical_json_bounded(
            &schema,
            super::schema::AUTOMATION_WEBHOOK_SCHEMA_MAX_BYTES,
            "Automation webhook request schema",
        )?;
        let digest = Sha256Digest::parse(sha256_digest(&canonical))?;
        DigestBoundJsonSchemaValidator::new(digest.as_str(), schema.clone())?;
        self.schemas.write().await.insert(digest.clone(), schema);
        Ok(digest)
    }
}

#[async_trait]
impl IAutomationWebhookSchemaRegistry for InMemoryAutomationWebhookSchemaRegistry {
    async fn resolve(&self, digest: &Sha256Digest) -> Result<Option<Value>, String> {
        Ok(self.schemas.read().await.get(digest).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{
        AutomationSubscriptionReferenceV1, AutomationWebhookEndpointStateV1,
        AutomationWebhookSecretReferenceV1, AutomationWebhookSignatureAlgorithmV1,
        AutomationWebhookSignatureV1,
    };
    use chrono::DateTime;
    use serde_json::json;
    use uuid::Uuid;

    fn endpoint(schema_digest: &str) -> AutomationWebhookEndpointV1 {
        AutomationWebhookEndpointV1 {
            schema: AutomationWebhookEndpointV1::SCHEMA.into(),
            endpoint_id: Uuid::from_u128(1),
            endpoint_key: "hook".into(),
            automation_id: Uuid::from_u128(2),
            revision_id: Uuid::from_u128(3),
            revision_digest: format!("sha256:{}", "a".repeat(64)),
            organization_id: Uuid::from_u128(4),
            project_id: Uuid::from_u128(5),
            environment_id: Uuid::from_u128(6),
            subscription: AutomationSubscriptionReferenceV1 {
                subscription_id: Uuid::from_u128(7),
                revision_digest: format!("sha256:{}", "b".repeat(64)),
            },
            signature_algorithm: AutomationWebhookSignatureAlgorithmV1::HmacSha256,
            signing_secret: AutomationWebhookSecretReferenceV1 {
                secret_id: Uuid::from_u128(8),
                version: 1,
            },
            request_schema_digest: schema_digest.into(),
            max_body_bytes: 1024,
            generation: 1,
            state: AutomationWebhookEndpointStateV1::Active,
            created_at: DateTime::from_timestamp(1_700_000_000, 0).expect("timestamp"),
            state_changed_at: None,
        }
    }

    #[tokio::test]
    async fn registry_returns_only_the_exact_registered_digest() {
        let registry = InMemoryAutomationWebhookSchemaRegistry::new();
        let digest = registry
            .register(json!({"type": "object", "required": ["release"]}))
            .await
            .expect("schema");
        assert!(registry.resolve(&digest).await.expect("lookup").is_some());
        let other = Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("digest");
        assert!(registry.resolve(&other).await.expect("lookup").is_none());
    }

    #[tokio::test]
    async fn registry_rejects_documents_that_cannot_be_compiled() {
        let registry = InMemoryAutomationWebhookSchemaRegistry::new();
        assert!(registry.register(json!({"type": 7})).await.is_err());
    }

    #[tokio::test]
    async fn registry_backed_validator_uses_the_endpoint_digest() {
        let registry = Arc::new(InMemoryAutomationWebhookSchemaRegistry::new());
        let digest = registry
            .register(json!({
                "type": "object",
                "required": ["release"],
                "properties": {"release": {"type": "string"}}
            }))
            .await
            .expect("schema");
        let endpoint = endpoint(digest.as_str());
        let request = AutomationWebhookRequestV1::from_json(
            &endpoint,
            Uuid::from_u128(9),
            AutomationWebhookSignatureV1 {
                algorithm: AutomationWebhookSignatureAlgorithmV1::HmacSha256,
                key_version: 1,
                value: format!("hmac-sha256:{}", "a".repeat(64)),
            },
            "application/json",
            br#"{"release":"stable"}"#,
            DateTime::from_timestamp(1_700_000_001, 0).expect("timestamp"),
        )
        .expect("request");
        RegistryBackedAutomationWebhookSchemaValidator::new(registry)
            .validate(&endpoint, &request)
            .await
            .expect("registry-backed validation");
    }
}
