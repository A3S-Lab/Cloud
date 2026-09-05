use a3s_cloud_contracts::{
    AutomationInvocationEnvelopeV1, AutomationRevisionV1, AutomationWebhookDeliveryReceiptV1,
    AutomationWebhookEndpointV1, AutomationWebhookRequestV1,
};
use uuid::Uuid;

/// The endpoint projection and the exact immutable revision it serves.
///
/// Keeping the revision beside the endpoint prevents a later lookup from
/// silently resolving a mutable "latest" Automation.  A PostgreSQL adapter
/// will persist the canonical ACL and digest and restore this same value
/// object before returning it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationWebhookEndpointRecord {
    pub endpoint: AutomationWebhookEndpointV1,
    pub revision: AutomationRevisionV1,
}

impl AutomationWebhookEndpointRecord {
    pub fn new(
        endpoint: AutomationWebhookEndpointV1,
        revision: AutomationRevisionV1,
    ) -> Result<Self, String> {
        endpoint.validate_for_revision(&revision)?;
        Ok(Self { endpoint, revision })
    }

    pub fn validate(&self) -> Result<(), String> {
        self.endpoint.validate_for_revision(&self.revision)
    }

    pub fn endpoint_id(&self) -> Uuid {
        self.endpoint.endpoint_id
    }

    pub fn scope_key(&self) -> (Uuid, Uuid, Uuid, String) {
        (
            self.endpoint.organization_id,
            self.endpoint.project_id,
            self.endpoint.environment_id,
            self.endpoint.endpoint_key.clone(),
        )
    }
}

/// Durable delivery state.  Request capture is bounded by the contract and
/// contains no secret material; the signing secret remains an identity/version
/// reference on the endpoint.  Rejected deliveries deliberately carry no
/// invocation envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationWebhookDeliveryRecord {
    pub request: AutomationWebhookRequestV1,
    pub receipt: AutomationWebhookDeliveryReceiptV1,
    pub invocation: Option<AutomationInvocationEnvelopeV1>,
}

impl AutomationWebhookDeliveryRecord {
    pub fn new(
        request: AutomationWebhookRequestV1,
        receipt: AutomationWebhookDeliveryReceiptV1,
        invocation: Option<AutomationInvocationEnvelopeV1>,
        endpoint: &AutomationWebhookEndpointV1,
        revision: &AutomationRevisionV1,
    ) -> Result<Self, String> {
        receipt.validate_for_endpoint(endpoint, revision)?;
        request.validate_for_endpoint(endpoint)?;
        match (&receipt.invocation_id, &invocation) {
            (Some(invocation_id), Some(invocation)) => {
                invocation.validate_for_revision(revision)?;
                if invocation.invocation_id != *invocation_id {
                    return Err(
                        "webhook delivery invocation identity does not match receipt".into(),
                    );
                }
            }
            (None, None) => {}
            _ => return Err("webhook delivery invocation presence does not match receipt".into()),
        }
        Ok(Self {
            request,
            receipt,
            invocation,
        })
    }

    pub fn key(&self) -> (Uuid, Uuid) {
        (self.request.endpoint_id, self.request.delivery_id)
    }
}
