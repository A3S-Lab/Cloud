use crate::modules::automations::domain::IAutomationWebhookSchemaValidator;
use crate::modules::shared_kernel::domain::{canonical_json_bounded, sha256_digest, Sha256Digest};
use async_trait::async_trait;
use jsonschema::{Retrieve, Uri, Validator};
use serde_json::Value;
use std::fmt;
use std::sync::Arc;

/// Maximum canonical size of one schema document accepted by the component
/// adapter.  The schema registry (when one is introduced) remains responsible
/// for selecting and publishing the document; this adapter only evaluates the
/// already selected digest/document pair.
pub const AUTOMATION_WEBHOOK_SCHEMA_MAX_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy)]
struct JsonBounds {
    maximum_depth: usize,
    maximum_nodes: usize,
    maximum_container_elements: usize,
    maximum_key_bytes: usize,
    label: &'static str,
}

const SCHEMA_BOUNDS: JsonBounds = JsonBounds {
    maximum_depth: 64,
    maximum_nodes: 8_192,
    maximum_container_elements: 4_096,
    maximum_key_bytes: 4 * 1024,
    label: "Automation webhook request schema",
};

const PAYLOAD_BOUNDS: JsonBounds = JsonBounds {
    maximum_depth: 64,
    maximum_nodes: 8_192,
    maximum_container_elements: 4_096,
    maximum_key_bytes: 4 * 1024,
    label: "Automation webhook payload",
};

/// A compiled JSON Schema that is permanently bound to one canonical digest.
///
/// The type is intentionally a component adapter rather than a registry.  It
/// owns neither schema lookup nor publication and accepts no URL, path, or
/// caller-selected fallback.  Callers must obtain the exact document/digest
/// pair from the owning schema authority before constructing it.
#[derive(Clone)]
pub struct DigestBoundJsonSchemaValidator {
    schema_digest: Sha256Digest,
    validator: Arc<Validator>,
}

impl fmt::Debug for DigestBoundJsonSchemaValidator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DigestBoundJsonSchemaValidator")
            .field("schema_digest", &self.schema_digest)
            .finish_non_exhaustive()
    }
}

impl DigestBoundJsonSchemaValidator {
    /// Compile one self-contained schema and bind it to its canonical digest.
    ///
    /// Schema bytes are canonicalized with the same sorted-object convention
    /// used by Cloud contracts.  External `$ref` retrieval is deliberately
    /// disabled so constructing this adapter cannot perform network or local
    /// filesystem I/O.
    pub fn new(schema_digest: impl Into<String>, schema: Value) -> Result<Self, String> {
        let schema_digest = Sha256Digest::parse(schema_digest.into())
            .map_err(|_| "Automation webhook request schema digest is invalid".to_owned())?;
        let mut nodes = 0;
        validate_json_bounds(&schema, 0, &mut nodes, SCHEMA_BOUNDS)?;
        let canonical = canonical_json_bounded(
            &schema,
            AUTOMATION_WEBHOOK_SCHEMA_MAX_BYTES,
            "Automation webhook request schema",
        )?;
        if sha256_digest(&canonical) != schema_digest.as_str() {
            return Err(
                "Automation webhook request schema digest does not match the supplied schema"
                    .into(),
            );
        }

        let validator = jsonschema::options()
            .with_retriever(NoExternalSchemaRetriever)
            .build(&schema)
            .map_err(|_| {
                "Automation webhook request schema cannot be compiled by the fixed validator"
                    .to_owned()
            })?;
        Ok(Self {
            schema_digest,
            validator: Arc::new(validator),
        })
    }

    pub fn schema_digest(&self) -> &str {
        self.schema_digest.as_str()
    }
}

#[async_trait]
impl IAutomationWebhookSchemaValidator for DigestBoundJsonSchemaValidator {
    async fn validate(
        &self,
        endpoint: &a3s_cloud_contracts::AutomationWebhookEndpointV1,
        request: &a3s_cloud_contracts::AutomationWebhookRequestV1,
    ) -> Result<(), String> {
        endpoint.validate().map_err(|error| {
            format!("Automation webhook endpoint is invalid for schema validation: {error}")
        })?;
        request
            .validate_for_endpoint(endpoint)
            .map_err(|error| format!("Automation webhook request is invalid: {error}"))?;
        if endpoint.request_schema_digest != self.schema_digest.as_str()
            || request.request_schema_digest != self.schema_digest.as_str()
        {
            return Err(
                "Automation webhook request schema digest is not bound to this validator".into(),
            );
        }
        let mut nodes = 0;
        validate_json_bounds(&request.payload, 0, &mut nodes, PAYLOAD_BOUNDS)?;
        if self.validator.is_valid(&request.payload) {
            Ok(())
        } else {
            Err("Automation webhook payload does not satisfy its request schema".into())
        }
    }
}

/// A retriever that always fails.  Local JSON pointers remain available to the
/// compiled validator; only out-of-document resources are denied.
#[derive(Debug, Clone, Copy)]
struct NoExternalSchemaRetriever;

impl Retrieve for NoExternalSchemaRetriever {
    fn retrieve(
        &self,
        _uri: &Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err("external Automation webhook schema retrieval is disabled".into())
    }
}

fn validate_json_bounds(
    value: &Value,
    depth: usize,
    nodes: &mut usize,
    bounds: JsonBounds,
) -> Result<(), String> {
    if depth > bounds.maximum_depth {
        return Err(format!("{} exceeds its nesting bound", bounds.label));
    }
    *nodes = nodes
        .checked_add(1)
        .ok_or_else(|| format!("{} exceeds its node bound", bounds.label))?;
    if *nodes > bounds.maximum_nodes {
        return Err(format!("{} exceeds its node bound", bounds.label));
    }
    match value {
        Value::Array(values) => {
            if values.len() > bounds.maximum_container_elements {
                return Err(format!("{} exceeds its array-element bound", bounds.label));
            }
            for value in values {
                validate_json_bounds(value, depth + 1, nodes, bounds)?;
            }
        }
        Value::Object(object) => {
            if object.len() > bounds.maximum_container_elements {
                return Err(format!("{} exceeds its object-member bound", bounds.label));
            }
            for (key, value) in object {
                if key.len() > bounds.maximum_key_bytes || key.chars().any(char::is_control) {
                    return Err(format!("{} contains an invalid object key", bounds.label));
                }
                validate_json_bounds(value, depth + 1, nodes, bounds)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{
        AutomationDefinitionV1, AutomationRevisionV1, AutomationTriggerV1,
        AutomationWebhookEndpointV1, AutomationWebhookRequestV1,
        AutomationWebhookSecretReferenceV1, AutomationWebhookSignatureAlgorithmV1,
        AutomationWebhookSignatureV1,
    };
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use uuid::Uuid;

    const WEBHOOK_DEFINITION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-webhook.acl"
    ));

    fn id(value: u16) -> Uuid {
        Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_0000_u128 + u128::from(value))
    }

    fn timestamp(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("timestamp")
            .with_timezone(&Utc)
    }

    fn digest(schema: &Value) -> String {
        sha256_digest(
            &canonical_json_bounded(schema, AUTOMATION_WEBHOOK_SCHEMA_MAX_BYTES, "test schema")
                .expect("canonical schema"),
        )
    }

    fn schema() -> Value {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$defs": {
                "release": {"type": "string", "minLength": 1}
            },
            "type": "object",
            "required": ["release"],
            "properties": {
                "release": {"$ref": "#/$defs/release"}
            },
            "additionalProperties": false
        })
    }

    fn endpoint_and_request(
        schema_digest: &str,
        body: &[u8],
        seed: u16,
    ) -> (AutomationWebhookEndpointV1, AutomationWebhookRequestV1) {
        let definition = AutomationDefinitionV1::parse_acl(WEBHOOK_DEFINITION).expect("definition");
        let mut spec = definition.spec().clone();
        let AutomationTriggerV1::Webhook(trigger) = &mut spec.trigger else {
            panic!("webhook trigger")
        };
        trigger.request_schema_digest = schema_digest.into();
        let definition = AutomationDefinitionV1::from_spec(spec).expect("definition with schema");
        let revision =
            AutomationRevisionV1::from_definition(id(seed), 1, None, definition.spec().clone())
                .expect("revision");
        let endpoint = AutomationWebhookEndpointV1::for_revision(
            id(seed + 1),
            "release-hook",
            AutomationWebhookSecretReferenceV1 {
                secret_id: id(seed + 2),
                version: 4,
            },
            4096,
            &revision,
            timestamp("2026-09-05T00:00:00.000Z"),
        )
        .expect("endpoint");
        let request = AutomationWebhookRequestV1::from_json(
            &endpoint,
            id(seed + 3),
            AutomationWebhookSignatureV1 {
                algorithm: AutomationWebhookSignatureAlgorithmV1::HmacSha256,
                key_version: endpoint.signing_secret.version,
                value: format!("hmac-sha256:{}", "a".repeat(64)),
            },
            "application/json",
            body,
            timestamp("2026-09-05T00:00:01.000Z"),
        )
        .expect("request");
        (endpoint, request)
    }

    #[tokio::test]
    async fn compiles_and_validates_a_digest_bound_schema_with_local_references() {
        let schema = schema();
        let schema_digest = digest(&schema);
        let validator =
            DigestBoundJsonSchemaValidator::new(schema_digest.clone(), schema).expect("validator");
        assert_eq!(validator.schema_digest(), schema_digest);
        let (endpoint, request) =
            endpoint_and_request(&schema_digest, br#"{"release":"stable"}"#, 0x100);
        validator
            .validate(&endpoint, &request)
            .await
            .expect("schema validation");
    }

    #[tokio::test]
    async fn rejects_an_endpoint_bound_to_a_different_schema_digest() {
        let bound_schema = schema();
        let bound_digest = digest(&bound_schema);
        let validator =
            DigestBoundJsonSchemaValidator::new(bound_digest, bound_schema).expect("validator");
        let endpoint_schema = json!({"type": "object"});
        let endpoint_digest = digest(&endpoint_schema);
        let (endpoint, request) =
            endpoint_and_request(&endpoint_digest, br#"{"release":"stable"}"#, 0x110);
        let error = validator
            .validate(&endpoint, &request)
            .await
            .expect_err("digest drift");
        assert_eq!(
            error,
            "Automation webhook request schema digest is not bound to this validator"
        );
    }

    #[test]
    fn rejects_digest_drift_invalid_schemas_and_external_references() {
        let schema = schema();
        let wrong = format!("sha256:{}", "0".repeat(64));
        assert_eq!(
            DigestBoundJsonSchemaValidator::new(wrong, schema.clone())
                .expect_err("digest drift")
                .as_str(),
            "Automation webhook request schema digest does not match the supplied schema"
        );

        let external = json!({"$ref": "https://schemas.example.invalid/request.json"});
        let error = DigestBoundJsonSchemaValidator::new(digest(&external), external)
            .expect_err("external ref");
        assert_eq!(
            error,
            "Automation webhook request schema cannot be compiled by the fixed validator"
        );

        let invalid = json!({"type": 7});
        let error = DigestBoundJsonSchemaValidator::new(digest(&invalid), invalid)
            .expect_err("invalid schema");
        assert_eq!(
            error,
            "Automation webhook request schema cannot be compiled by the fixed validator"
        );
    }

    #[tokio::test]
    async fn rejects_payload_mismatch_without_returning_validator_details() {
        let schema = schema();
        let schema_digest = digest(&schema);
        let validator =
            DigestBoundJsonSchemaValidator::new(schema_digest.clone(), schema).expect("validator");
        let (endpoint, request) = endpoint_and_request(&schema_digest, br#"{"release":7}"#, 0x130);
        let error = validator
            .validate(&endpoint, &request)
            .await
            .expect_err("schema mismatch");
        assert_eq!(
            error,
            "Automation webhook payload does not satisfy its request schema"
        );
        assert!(!error.contains("release"));
    }
}
