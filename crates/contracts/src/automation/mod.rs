mod codec;
mod definition;
mod events;
mod invocation;
mod validation;
mod webhook;

pub use definition::{
    AutomationApplicationTargetV1, AutomationAuthorizationPolicyV1, AutomationConcurrencyModeV1,
    AutomationConcurrencyPolicyV1, AutomationDeduplicationPolicyV1, AutomationDeduplicationScopeV1,
    AutomationDefinitionSpecV1, AutomationDefinitionV1, AutomationEventTriggerV1,
    AutomationMisfireModeV1, AutomationMisfirePolicyV1, AutomationRevisionSpecV1,
    AutomationRevisionV1, AutomationScheduleTriggerV1, AutomationSubscriptionReferenceV1,
    AutomationTargetKindV1, AutomationTargetV1, AutomationTaskTargetV1, AutomationTriggerPolicyV1,
    AutomationTriggerV1, AutomationWebhookTriggerV1, AutomationWorkflowTargetV1,
    AUTOMATION_DEFINITION_MAX_ACL_BYTES, AUTOMATION_DEFINITION_SCHEMA_V1,
    AUTOMATION_MAX_CONCURRENCY, AUTOMATION_MAX_DEDUPLICATION_TEMPLATE_BYTES,
    AUTOMATION_MAX_DEDUPLICATION_WINDOW_MS, AUTOMATION_MAX_MISFIRE_GRACE_MS,
    AUTOMATION_MAX_NAME_BYTES, AUTOMATION_REVISION_SCHEMA_V1,
};
pub use events::{
    AutomationAuditActionV1, AutomationAuditRecordV1, AutomationOutboxEventKindV1,
    AutomationOutboxMessageV1, AUTOMATION_AUDIT_SCHEMA_V1, AUTOMATION_OUTBOX_SCHEMA_V1,
};
pub use invocation::{
    AutomationInvocationAuthorizationV1, AutomationInvocationEnvelopeV1,
    AutomationInvocationInputV1, AutomationInvocationOriginV1,
    AUTOMATION_INVOCATION_ENVELOPE_MAX_BYTES, AUTOMATION_INVOCATION_INLINE_MAX_BYTES,
    AUTOMATION_INVOCATION_SCHEMA_V1,
};
pub use webhook::{
    AutomationWebhookAdmissionDecisionV1, AutomationWebhookDeliveryReceiptV1,
    AutomationWebhookEndpointStateV1, AutomationWebhookEndpointV1,
    AutomationWebhookRejectionReasonV1, AutomationWebhookRequestV1,
    AutomationWebhookSecretReferenceV1, AutomationWebhookSignatureAlgorithmV1,
    AutomationWebhookSignatureV1, AUTOMATION_WEBHOOK_ENDPOINT_SCHEMA_V1,
    AUTOMATION_WEBHOOK_MAX_BODY_BYTES, AUTOMATION_WEBHOOK_MAX_CAPTURE_BASE64_BYTES,
    AUTOMATION_WEBHOOK_MAX_CONTENT_TYPE_BYTES, AUTOMATION_WEBHOOK_MAX_ENDPOINT_KEY_BYTES,
    AUTOMATION_WEBHOOK_MAX_SIGNATURE_BYTES, AUTOMATION_WEBHOOK_RECEIPT_SCHEMA_V1,
    AUTOMATION_WEBHOOK_REQUEST_SCHEMA_V1,
};

/// Short aliases keep the public contract ergonomic while the versioned names
/// remain available for wire/schema migrations.
pub type AutomationDefinition = AutomationDefinitionV1;
pub type AutomationRevision = AutomationRevisionV1;
pub type AutomationInvocationEnvelope = AutomationInvocationEnvelopeV1;
