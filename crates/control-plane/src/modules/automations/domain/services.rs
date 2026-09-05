use a3s_cloud_contracts::{AutomationWebhookEndpointV1, AutomationWebhookRequestV1};
use async_trait::async_trait;

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
