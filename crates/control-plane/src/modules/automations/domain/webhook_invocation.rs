use a3s_cloud_contracts::{
    AutomationInvocationAuthorizationV1, AutomationInvocationEnvelopeV1,
    AutomationInvocationInputV1, AutomationInvocationOriginV1, AutomationRevisionV1,
    AutomationTriggerV1, AutomationWebhookEndpointV1, AutomationWebhookRequestV1,
    AUTOMATION_WEBHOOK_EVENT_KEY,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Inputs for constructing one deterministic invocation from a captured
/// webhook request and its exact endpoint/revision projection.
///
/// Capture, signature verification, schema validation, admission, and target
/// execution remain separate authorities. This factory only creates the
/// common invocation envelope after binding every immutable identity.
pub struct AutomationWebhookInvocationRequest<'a> {
    pub endpoint: &'a AutomationWebhookEndpointV1,
    pub revision: &'a AutomationRevisionV1,
    pub request: &'a AutomationWebhookRequestV1,
    pub invocation_id: Uuid,
    pub requested_at: DateTime<Utc>,
    pub authorization: AutomationInvocationAuthorizationV1,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AutomationWebhookInvocationFactory;

impl AutomationWebhookInvocationFactory {
    pub fn build(
        input: AutomationWebhookInvocationRequest<'_>,
    ) -> Result<AutomationInvocationEnvelopeV1, String> {
        input.endpoint.validate_for_revision(input.revision)?;
        input.request.validate_for_endpoint(input.endpoint)?;
        let definition = &input.revision.spec().definition;
        let subscription = match &definition.trigger {
            AutomationTriggerV1::Webhook(trigger) => trigger.subscription.clone(),
            _ => {
                return Err(
                    "Automation webhook invocation requires a webhook-triggered revision".into(),
                )
            }
        };
        if input.request.received_at > input.requested_at {
            return Err(
                "Automation webhook invocation cannot be requested before request receipt".into(),
            );
        }
        let origin = AutomationInvocationOriginV1::Event {
            event_id: input.request.delivery_id,
            event_key: AUTOMATION_WEBHOOK_EVENT_KEY.into(),
            event_digest: input.request.body_digest.clone(),
            observed_at: input.request.received_at,
        };
        let deduplication_key = definition.policy.deduplication.render_key(
            definition.automation_id,
            input.revision.spec().revision_id,
            &origin,
            Some(subscription.subscription_id),
        )?;
        let envelope = AutomationInvocationEnvelopeV1 {
            schema: AutomationInvocationEnvelopeV1::SCHEMA.into(),
            invocation_id: input.invocation_id,
            automation_id: definition.automation_id,
            automation_revision_id: input.revision.spec().revision_id,
            automation_revision_digest: input.revision.digest().into(),
            organization_id: definition.organization_id,
            project_id: definition.project_id,
            environment_id: definition.environment_id,
            target: definition.target.clone(),
            origin,
            subscription: Some(subscription),
            deduplication_key,
            input: AutomationInvocationInputV1::inline_json(input.request.payload.clone())?,
            authorization: input.authorization,
            requested_at: input.requested_at,
            correlation_id: input.correlation_id,
            causation_id: input.causation_id,
        };
        envelope.validate_for_revision(input.revision)?;
        Ok(envelope)
    }
}

#[cfg(test)]
#[path = "webhook_invocation_tests.rs"]
mod tests;
