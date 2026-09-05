use super::definition::{
    AutomationRevisionV1, AutomationSubscriptionReferenceV1, AutomationTriggerV1,
};
use super::invocation::AutomationInvocationInputV1;
use super::invocation::{AutomationInvocationEnvelopeV1, AutomationInvocationOriginV1};
use super::validation::{
    canonical_json, json_digest, validate_digest, validate_media_type, validate_timestamp,
    validate_uuid, MAX_SAFE_INTEGER,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const AUTOMATION_WEBHOOK_ENDPOINT_SCHEMA_V1: &str = "cloud.automation.webhook.endpoint.v1";
pub const AUTOMATION_WEBHOOK_REQUEST_SCHEMA_V1: &str = "cloud.automation.webhook.request.v1";
pub const AUTOMATION_WEBHOOK_RECEIPT_SCHEMA_V1: &str = "cloud.automation.webhook.receipt.v1";
pub const AUTOMATION_WEBHOOK_MAX_ENDPOINT_KEY_BYTES: usize = 128;
pub const AUTOMATION_WEBHOOK_MAX_BODY_BYTES: u64 = 1024 * 1024;
pub const AUTOMATION_WEBHOOK_MAX_CAPTURE_BASE64_BYTES: usize = 4 * 1024 * 1024;
pub const AUTOMATION_WEBHOOK_MAX_CONTENT_TYPE_BYTES: usize = 127;
pub const AUTOMATION_WEBHOOK_MAX_SIGNATURE_BYTES: usize = 80;

/// A reference to the signing secret. Secret material is never part of an
/// Automation contract; Secrets resolves this identity at the admission edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationWebhookSecretReferenceV1 {
    pub secret_id: Uuid,
    pub version: u64,
}

impl AutomationWebhookSecretReferenceV1 {
    pub fn validate(&self) -> Result<(), String> {
        validate_uuid("Automation webhook secret ID", self.secret_id)?;
        if self.version == 0 || self.version > MAX_SAFE_INTEGER {
            return Err("Automation webhook secret version is outside its bound".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWebhookSignatureAlgorithmV1 {
    HmacSha256,
}

impl AutomationWebhookSignatureAlgorithmV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HmacSha256 => "hmac_sha256",
        }
    }
}

/// Normalized signature facts. Verification happens in infrastructure; this
/// type deliberately carries no key or secret material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationWebhookSignatureV1 {
    pub algorithm: AutomationWebhookSignatureAlgorithmV1,
    pub key_version: u64,
    pub value: String,
}

impl AutomationWebhookSignatureV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.key_version == 0 || self.key_version > MAX_SAFE_INTEGER {
            return Err("Automation webhook signature key version is outside its bound".into());
        }
        if self.value.len() > AUTOMATION_WEBHOOK_MAX_SIGNATURE_BYTES
            || self.value.len() != 76
            || !self.value.starts_with("hmac-sha256:")
            || !self.value[12..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(
                "Automation webhook signature must use canonical HMAC-SHA256 syntax".into(),
            );
        }
        if !matches!(
            self.algorithm,
            AutomationWebhookSignatureAlgorithmV1::HmacSha256
        ) {
            return Err("Automation webhook signature algorithm is unsupported".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWebhookEndpointStateV1 {
    Active,
    Disabled,
    Revoked,
}

impl AutomationWebhookEndpointStateV1 {
    pub const fn is_accepting(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// Immutable endpoint identity and mutable lifecycle projection for one exact
/// Automation revision. The route key is opaque and never contains a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationWebhookEndpointV1 {
    pub schema: String,
    pub endpoint_id: Uuid,
    pub endpoint_key: String,
    pub automation_id: Uuid,
    pub revision_id: Uuid,
    pub revision_digest: String,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub subscription: AutomationSubscriptionReferenceV1,
    pub signature_algorithm: AutomationWebhookSignatureAlgorithmV1,
    pub signing_secret: AutomationWebhookSecretReferenceV1,
    pub request_schema_digest: String,
    pub max_body_bytes: u64,
    pub generation: u64,
    pub state: AutomationWebhookEndpointStateV1,
    pub created_at: DateTime<Utc>,
    pub state_changed_at: Option<DateTime<Utc>>,
}

impl AutomationWebhookEndpointV1 {
    pub const SCHEMA: &'static str = AUTOMATION_WEBHOOK_ENDPOINT_SCHEMA_V1;

    pub fn for_revision(
        endpoint_id: Uuid,
        endpoint_key: impl Into<String>,
        signing_secret: AutomationWebhookSecretReferenceV1,
        max_body_bytes: u64,
        revision: &AutomationRevisionV1,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        revision.validate()?;
        let definition = &revision.spec().definition;
        let AutomationTriggerV1::Webhook(trigger) = &definition.trigger else {
            return Err("Automation webhook endpoint requires a webhook trigger".into());
        };
        let endpoint = Self {
            schema: Self::SCHEMA.into(),
            endpoint_id,
            endpoint_key: endpoint_key.into(),
            automation_id: definition.automation_id,
            revision_id: revision.spec().revision_id,
            revision_digest: revision.digest().into(),
            organization_id: definition.organization_id,
            project_id: definition.project_id,
            environment_id: definition.environment_id,
            subscription: trigger.subscription.clone(),
            signature_algorithm: AutomationWebhookSignatureAlgorithmV1::HmacSha256,
            signing_secret,
            request_schema_digest: trigger.request_schema_digest.clone(),
            max_body_bytes,
            generation: 1,
            state: AutomationWebhookEndpointStateV1::Active,
            created_at,
            state_changed_at: None,
        };
        endpoint.validate_for_revision(revision)?;
        Ok(endpoint)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Automation webhook endpoint schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Automation webhook endpoint ID", self.endpoint_id)?;
        validate_endpoint_key(&self.endpoint_key)?;
        validate_uuid(
            "Automation webhook endpoint Automation ID",
            self.automation_id,
        )?;
        validate_uuid("Automation webhook endpoint revision ID", self.revision_id)?;
        validate_digest(
            "Automation webhook endpoint revision digest",
            &self.revision_digest,
        )?;
        validate_uuid(
            "Automation webhook endpoint organization ID",
            self.organization_id,
        )?;
        validate_uuid("Automation webhook endpoint project ID", self.project_id)?;
        validate_uuid(
            "Automation webhook endpoint environment ID",
            self.environment_id,
        )?;
        self.subscription.validate()?;
        if !matches!(
            self.signature_algorithm,
            AutomationWebhookSignatureAlgorithmV1::HmacSha256
        ) {
            return Err(
                "Automation webhook endpoint uses an unsupported signature algorithm".into(),
            );
        }
        self.signing_secret.validate()?;
        validate_digest(
            "Automation webhook endpoint request schema digest",
            &self.request_schema_digest,
        )?;
        if self.max_body_bytes == 0 || self.max_body_bytes > AUTOMATION_WEBHOOK_MAX_BODY_BYTES {
            return Err("Automation webhook endpoint body bound is outside its limit".into());
        }
        if self.generation == 0 || self.generation > MAX_SAFE_INTEGER {
            return Err("Automation webhook endpoint generation is outside its bound".into());
        }
        validate_timestamp("Automation webhook endpoint created_at", self.created_at)?;
        match (self.state, self.state_changed_at) {
            (AutomationWebhookEndpointStateV1::Active, None) => {}
            (AutomationWebhookEndpointStateV1::Active, Some(_)) => {
                return Err(
                    "active Automation webhook endpoint cannot have a state timestamp".into(),
                )
            }
            (_, Some(changed_at)) => {
                validate_timestamp("Automation webhook endpoint state_changed_at", changed_at)?;
                if changed_at < self.created_at {
                    return Err("Automation webhook endpoint state change predates creation".into());
                }
            }
            (_, None) => {
                return Err(
                    "inactive Automation webhook endpoint requires a state timestamp".into(),
                )
            }
        }
        Ok(())
    }

    pub fn validate_for_revision(&self, revision: &AutomationRevisionV1) -> Result<(), String> {
        self.validate()?;
        revision.validate()?;
        let definition = &revision.spec().definition;
        let AutomationTriggerV1::Webhook(trigger) = &definition.trigger else {
            return Err("Automation webhook endpoint revision is not webhook-triggered".into());
        };
        if self.automation_id != definition.automation_id
            || self.revision_id != revision.spec().revision_id
            || self.revision_digest != revision.digest()
            || self.organization_id != definition.organization_id
            || self.project_id != definition.project_id
            || self.environment_id != definition.environment_id
            || self.subscription != trigger.subscription
            || self.request_schema_digest != trigger.request_schema_digest
        {
            return Err("Automation webhook endpoint is not bound to the exact revision".into());
        }
        Ok(())
    }

    pub fn disable(&mut self, changed_at: DateTime<Utc>) -> Result<(), String> {
        self.validate()?;
        validate_timestamp("Automation webhook endpoint disable time", changed_at)?;
        if self.state != AutomationWebhookEndpointStateV1::Active {
            return Err("only an active Automation webhook endpoint can be disabled".into());
        }
        if changed_at < self.created_at {
            return Err("Automation webhook endpoint disable time predates creation".into());
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| "Automation webhook endpoint generation overflowed".to_owned())?;
        self.state = AutomationWebhookEndpointStateV1::Disabled;
        self.state_changed_at = Some(changed_at);
        self.generation = generation;
        self.validate()
    }

    pub fn revoke(&mut self, changed_at: DateTime<Utc>) -> Result<(), String> {
        self.validate()?;
        validate_timestamp("Automation webhook endpoint revoke time", changed_at)?;
        if self.state == AutomationWebhookEndpointStateV1::Revoked {
            return Err("Automation webhook endpoint is already revoked".into());
        }
        if changed_at < self.created_at {
            return Err("Automation webhook endpoint revoke time predates creation".into());
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| "Automation webhook endpoint generation overflowed".to_owned())?;
        self.state = AutomationWebhookEndpointStateV1::Revoked;
        self.state_changed_at = Some(changed_at);
        self.generation = generation;
        self.validate()
    }

    pub fn enable(&mut self, changed_at: DateTime<Utc>) -> Result<(), String> {
        self.validate()?;
        validate_timestamp("Automation webhook endpoint enable time", changed_at)?;
        if self.state != AutomationWebhookEndpointStateV1::Disabled {
            return Err("only a disabled Automation webhook endpoint can be enabled".into());
        }
        if changed_at < self.created_at {
            return Err("Automation webhook endpoint enable time predates creation".into());
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| "Automation webhook endpoint generation overflowed".to_owned())?;
        self.state = AutomationWebhookEndpointStateV1::Active;
        self.state_changed_at = None;
        self.generation = generation;
        self.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationWebhookRequestV1 {
    pub schema: String,
    pub endpoint_id: Uuid,
    pub delivery_id: Uuid,
    pub request_schema_digest: String,
    pub signature: AutomationWebhookSignatureV1,
    pub content_type: String,
    pub body_base64: String,
    pub body_size_bytes: u64,
    pub body_digest: String,
    pub payload: Value,
    pub payload_digest: String,
    pub received_at: DateTime<Utc>,
}

impl AutomationWebhookRequestV1 {
    pub const SCHEMA: &'static str = AUTOMATION_WEBHOOK_REQUEST_SCHEMA_V1;

    pub fn from_json(
        endpoint: &AutomationWebhookEndpointV1,
        delivery_id: Uuid,
        signature: AutomationWebhookSignatureV1,
        content_type: impl Into<String>,
        body: &[u8],
        received_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if body.len() > endpoint.max_body_bytes as usize {
            return Err("Automation webhook body exceeds endpoint bound".into());
        }
        let payload: Value = serde_json::from_slice(body)
            .map_err(|error| format!("Automation webhook body is not valid JSON: {error}"))?;
        let payload_bytes = canonical_json(&payload, endpoint.max_body_bytes as usize)?;
        let request = Self {
            schema: Self::SCHEMA.into(),
            endpoint_id: endpoint.endpoint_id,
            delivery_id,
            request_schema_digest: endpoint.request_schema_digest.clone(),
            signature,
            content_type: content_type.into(),
            body_base64: STANDARD.encode(body),
            body_size_bytes: body.len() as u64,
            body_digest: digest_bytes(body),
            payload,
            payload_digest: json_digest(&payload_bytes),
            received_at,
        };
        request.validate_for_endpoint(endpoint)?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Automation webhook request schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Automation webhook request endpoint ID", self.endpoint_id)?;
        validate_uuid("Automation webhook delivery ID", self.delivery_id)?;
        validate_digest(
            "Automation webhook request schema digest",
            &self.request_schema_digest,
        )?;
        self.signature.validate()?;
        validate_media_type("Automation webhook content type", &self.content_type)?;
        if self.content_type.len() > AUTOMATION_WEBHOOK_MAX_CONTENT_TYPE_BYTES {
            return Err("Automation webhook content type exceeds its bound".into());
        }
        if self.content_type != "application/json" {
            return Err("Automation webhook content type must be application/json".into());
        }
        if self.body_base64.is_empty()
            || self.body_base64.len() > AUTOMATION_WEBHOOK_MAX_CAPTURE_BASE64_BYTES
        {
            return Err("Automation webhook captured body is outside its bound".into());
        }
        let body = STANDARD
            .decode(&self.body_base64)
            .map_err(|_| "Automation webhook captured body is not canonical base64".to_owned())?;
        if STANDARD.encode(&body) != self.body_base64 {
            return Err("Automation webhook captured body is not canonical base64".into());
        }
        if self.body_size_bytes == 0
            || self.body_size_bytes > AUTOMATION_WEBHOOK_MAX_BODY_BYTES
            || self.body_size_bytes != body.len() as u64
            || self.body_digest != digest_bytes(&body)
        {
            return Err("Automation webhook body size and digest do not match".into());
        }
        let parsed: Value = serde_json::from_slice(&body).map_err(|error| {
            format!("Automation webhook captured body is not valid JSON: {error}")
        })?;
        let payload_bytes =
            canonical_json(&self.payload, AUTOMATION_WEBHOOK_MAX_BODY_BYTES as usize)?;
        if parsed != self.payload || self.payload_digest != json_digest(&payload_bytes) {
            return Err("Automation webhook payload and digest do not match captured body".into());
        }
        validate_timestamp("Automation webhook received_at", self.received_at)
    }

    pub fn validate_for_endpoint(
        &self,
        endpoint: &AutomationWebhookEndpointV1,
    ) -> Result<(), String> {
        self.validate()?;
        endpoint.validate()?;
        if self.endpoint_id != endpoint.endpoint_id
            || self.request_schema_digest != endpoint.request_schema_digest
            || self.body_size_bytes > endpoint.max_body_bytes
            || self.signature.algorithm != endpoint.signature_algorithm
            || self.signature.key_version != endpoint.signing_secret.version
        {
            return Err("Automation webhook request does not bind to the endpoint policy".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWebhookAdmissionDecisionV1 {
    Admitted,
    Replayed,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWebhookRejectionReasonV1 {
    EndpointDisabled,
    EndpointRevoked,
    InvalidSignature,
    BodyTooLarge,
    SchemaMismatch,
    DuplicateDeliveryConflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationWebhookDeliveryReceiptV1 {
    pub schema: String,
    pub receipt_id: Uuid,
    pub endpoint_id: Uuid,
    pub endpoint_generation: u64,
    pub delivery_id: Uuid,
    pub automation_id: Uuid,
    pub revision_id: Uuid,
    pub revision_digest: String,
    pub body_digest: String,
    pub decision: AutomationWebhookAdmissionDecisionV1,
    pub rejection_reason: Option<AutomationWebhookRejectionReasonV1>,
    pub invocation_id: Option<Uuid>,
    pub first_received_at: DateTime<Utc>,
    pub recorded_at: DateTime<Utc>,
    pub correlation_id: Uuid,
}

impl AutomationWebhookDeliveryReceiptV1 {
    pub const SCHEMA: &'static str = AUTOMATION_WEBHOOK_RECEIPT_SCHEMA_V1;

    pub fn admitted(
        receipt_id: Uuid,
        endpoint: &AutomationWebhookEndpointV1,
        revision: &AutomationRevisionV1,
        request: &AutomationWebhookRequestV1,
        invocation: &AutomationInvocationEnvelopeV1,
        recorded_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        endpoint.validate_for_revision(revision)?;
        request.validate_for_endpoint(endpoint)?;
        if !endpoint.state.is_accepting() {
            return Err("inactive Automation webhook endpoint cannot admit a delivery".into());
        }
        invocation.validate_for_revision(revision)?;
        if invocation.invocation_id.is_nil()
            || invocation.origin.event_id() != Some(request.delivery_id)
            || !matches!(
                &invocation.origin,
                AutomationInvocationOriginV1::Event { event_key, event_digest, .. }
                    if event_key == "automation.webhook.received" && event_digest == &request.body_digest
            )
        {
            return Err("Automation webhook invocation is not bound to the delivery".into());
        }
        if !matches!(
            &invocation.input,
            AutomationInvocationInputV1::InlineJson { value, .. } if value == &request.payload
        ) {
            return Err("Automation webhook invocation input is not the captured payload".into());
        }
        if invocation.requested_at < request.received_at {
            return Err("Automation webhook invocation predates request receipt".into());
        }
        let receipt = Self {
            schema: Self::SCHEMA.into(),
            receipt_id,
            endpoint_id: endpoint.endpoint_id,
            endpoint_generation: endpoint.generation,
            delivery_id: request.delivery_id,
            automation_id: revision.spec().definition.automation_id,
            revision_id: revision.spec().revision_id,
            revision_digest: revision.digest().into(),
            body_digest: request.body_digest.clone(),
            decision: AutomationWebhookAdmissionDecisionV1::Admitted,
            rejection_reason: None,
            invocation_id: Some(invocation.invocation_id),
            first_received_at: request.received_at,
            recorded_at,
            correlation_id: invocation.correlation_id,
        };
        receipt.validate_for_endpoint(endpoint, revision)?;
        Ok(receipt)
    }

    pub fn replay_of(
        receipt_id: Uuid,
        existing: &Self,
        endpoint: &AutomationWebhookEndpointV1,
        revision: &AutomationRevisionV1,
        request: &AutomationWebhookRequestV1,
        recorded_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        existing.validate_for_endpoint(endpoint, revision)?;
        request.validate_for_endpoint(endpoint)?;
        if existing.decision == AutomationWebhookAdmissionDecisionV1::Rejected
            || existing.delivery_id != request.delivery_id
            || existing.body_digest != request.body_digest
        {
            return Err("Automation webhook replay does not match the original delivery".into());
        }
        let replay = Self {
            schema: Self::SCHEMA.into(),
            receipt_id,
            endpoint_id: existing.endpoint_id,
            endpoint_generation: endpoint.generation,
            delivery_id: existing.delivery_id,
            automation_id: existing.automation_id,
            revision_id: existing.revision_id,
            revision_digest: existing.revision_digest.clone(),
            body_digest: existing.body_digest.clone(),
            decision: AutomationWebhookAdmissionDecisionV1::Replayed,
            rejection_reason: None,
            invocation_id: existing.invocation_id,
            first_received_at: existing.first_received_at,
            recorded_at,
            correlation_id: existing.correlation_id,
        };
        replay.validate_for_endpoint(endpoint, revision)?;
        Ok(replay)
    }

    pub fn rejected(
        receipt_id: Uuid,
        endpoint: &AutomationWebhookEndpointV1,
        revision: &AutomationRevisionV1,
        request: &AutomationWebhookRequestV1,
        reason: AutomationWebhookRejectionReasonV1,
        recorded_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        endpoint.validate_for_revision(revision)?;
        request.validate()?;
        if request.endpoint_id != endpoint.endpoint_id
            || (request.request_schema_digest != endpoint.request_schema_digest
                && !matches!(reason, AutomationWebhookRejectionReasonV1::SchemaMismatch))
            || (request.signature.algorithm != endpoint.signature_algorithm
                && !matches!(reason, AutomationWebhookRejectionReasonV1::InvalidSignature))
            || (request.signature.key_version != endpoint.signing_secret.version
                && !matches!(reason, AutomationWebhookRejectionReasonV1::InvalidSignature))
            || (request.body_size_bytes > endpoint.max_body_bytes
                && !matches!(reason, AutomationWebhookRejectionReasonV1::BodyTooLarge))
        {
            return Err(
                "Automation webhook rejected request does not bind to the endpoint policy".into(),
            );
        }
        if matches!(reason, AutomationWebhookRejectionReasonV1::SchemaMismatch)
            && request.request_schema_digest == endpoint.request_schema_digest
        {
            return Err("schema-mismatch rejection requires a different schema digest".into());
        }
        if matches!(reason, AutomationWebhookRejectionReasonV1::BodyTooLarge)
            && request.body_size_bytes <= endpoint.max_body_bytes
        {
            return Err("Automation webhook rejection reason does not match body size".into());
        }
        match reason {
            AutomationWebhookRejectionReasonV1::EndpointDisabled
                if endpoint.state != AutomationWebhookEndpointStateV1::Disabled =>
            {
                return Err("disabled rejection requires a disabled endpoint".into())
            }
            AutomationWebhookRejectionReasonV1::EndpointRevoked
                if endpoint.state != AutomationWebhookEndpointStateV1::Revoked =>
            {
                return Err("revoked rejection requires a revoked endpoint".into())
            }
            AutomationWebhookRejectionReasonV1::EndpointDisabled
            | AutomationWebhookRejectionReasonV1::EndpointRevoked
            | AutomationWebhookRejectionReasonV1::InvalidSignature
            | AutomationWebhookRejectionReasonV1::BodyTooLarge
            | AutomationWebhookRejectionReasonV1::SchemaMismatch
            | AutomationWebhookRejectionReasonV1::DuplicateDeliveryConflict => {}
        }
        let receipt = Self {
            schema: Self::SCHEMA.into(),
            receipt_id,
            endpoint_id: endpoint.endpoint_id,
            endpoint_generation: endpoint.generation,
            delivery_id: request.delivery_id,
            automation_id: revision.spec().definition.automation_id,
            revision_id: revision.spec().revision_id,
            revision_digest: revision.digest().into(),
            body_digest: request.body_digest.clone(),
            decision: AutomationWebhookAdmissionDecisionV1::Rejected,
            rejection_reason: Some(reason),
            invocation_id: None,
            first_received_at: request.received_at,
            recorded_at,
            correlation_id: receipt_id,
        };
        receipt.validate_for_endpoint(endpoint, revision)?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Automation webhook receipt schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Automation webhook receipt ID", self.receipt_id)?;
        validate_uuid("Automation webhook receipt endpoint ID", self.endpoint_id)?;
        if self.endpoint_generation == 0 || self.endpoint_generation > MAX_SAFE_INTEGER {
            return Err(
                "Automation webhook receipt endpoint generation is outside its bound".into(),
            );
        }
        validate_uuid("Automation webhook receipt delivery ID", self.delivery_id)?;
        validate_uuid(
            "Automation webhook receipt Automation ID",
            self.automation_id,
        )?;
        validate_uuid("Automation webhook receipt revision ID", self.revision_id)?;
        validate_digest(
            "Automation webhook receipt revision digest",
            &self.revision_digest,
        )?;
        validate_digest("Automation webhook receipt body digest", &self.body_digest)?;
        match (self.decision, self.rejection_reason, self.invocation_id) {
            (AutomationWebhookAdmissionDecisionV1::Rejected, Some(_), None) => {}
            (
                AutomationWebhookAdmissionDecisionV1::Admitted
                | AutomationWebhookAdmissionDecisionV1::Replayed,
                None,
                Some(invocation_id),
            ) => {
                validate_uuid("Automation webhook receipt invocation ID", invocation_id)?;
            }
            _ => return Err("Automation webhook receipt decision fields are inconsistent".into()),
        }
        validate_timestamp(
            "Automation webhook receipt first_received_at",
            self.first_received_at,
        )?;
        validate_timestamp("Automation webhook receipt recorded_at", self.recorded_at)?;
        if self.recorded_at < self.first_received_at {
            return Err("Automation webhook receipt recorded_at predates receipt".into());
        }
        validate_uuid(
            "Automation webhook receipt correlation ID",
            self.correlation_id,
        )
    }

    pub fn validate_for_endpoint(
        &self,
        endpoint: &AutomationWebhookEndpointV1,
        revision: &AutomationRevisionV1,
    ) -> Result<(), String> {
        self.validate()?;
        endpoint.validate_for_revision(revision)?;
        if self.endpoint_id != endpoint.endpoint_id
            || self.endpoint_generation > endpoint.generation
            || self.automation_id != revision.spec().definition.automation_id
            || self.revision_id != revision.spec().revision_id
            || self.revision_digest != revision.digest()
        {
            return Err("Automation webhook receipt is not bound to the endpoint revision".into());
        }
        Ok(())
    }
}

fn validate_endpoint_key(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > AUTOMATION_WEBHOOK_MAX_ENDPOINT_KEY_BYTES
        || value.contains(['/', '\\', '\0', '\r', '\n'])
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        return Err("Automation webhook endpoint key is not a bounded opaque key".into());
    }
    Ok(())
}

fn digest_bytes(value: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(value))
}
