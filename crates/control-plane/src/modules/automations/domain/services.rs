use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_cloud_contracts::{AutomationWebhookEndpointV1, AutomationWebhookRequestV1};
use async_trait::async_trait;
use serde_json::Value;

/// Infrastructure resolves the exact Secret version and verifies the
/// normalized signature fact.  The domain never receives key material.
#[async_trait]
pub trait IAutomationWebhookSignatureVerifier: Send + Sync {
    async fn verify(
        &self,
        endpoint: &AutomationWebhookEndpointV1,
        request: &AutomationWebhookRequestV1,
    ) -> Result<(), String>;
}

/// Schema evaluation is deliberately a port.  AUT0.2 freezes the schema digest
/// and payload capture; component adapters may evaluate an already-selected
/// document, while registry selection and runtime publication remain outside
/// this context. Admission cannot silently skip this port.
#[async_trait]
pub trait IAutomationWebhookSchemaValidator: Send + Sync {
    async fn validate(
        &self,
        endpoint: &AutomationWebhookEndpointV1,
        request: &AutomationWebhookRequestV1,
    ) -> Result<(), String>;
}

/// The owning schema authority selects one immutable document by its exact
/// digest. Automations never chooses a fallback, resolves a URL, or persists a
/// second schema registry.
#[async_trait]
pub trait IAutomationWebhookSchemaRegistry: Send + Sync {
    async fn resolve(&self, digest: &Sha256Digest) -> Result<Option<Value>, String>;
}
