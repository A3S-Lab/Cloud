//! Prompt-free Gateway→Cloud usage lifecycle event contract.
//!
//! Batch envelopes remain opaque at the transport layer (`payload_base64` +
//! digest). Cloud Inference validates every accepted payload against this
//! schema before durable ledger insertion so rollups never invent facts from
//! untyped bytes. Prompts, responses, and credential secrets are forbidden.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const INFERENCE_USAGE_LIFECYCLE_SCHEMA_V1: &str = "a3s.gateway.usage-lifecycle.v1";

const FORBIDDEN_PAYLOAD_KEYS: &[&str] = &[
    "prompt",
    "prompts",
    "messages",
    "input",
    "output",
    "response",
    "completion",
    "content",
    "authorization",
    "api_key",
    "secret",
];

/// Closed native inference endpoint identifiers carried on usage facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InferenceUsageEndpointV1 {
    Models,
    ChatCompletions,
    Completions,
    Embeddings,
}

/// Lifecycle event kinds Gateway appends to the durable spool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceUsageLifecycleKindV1 {
    RequestStarted,
    AttemptStarted,
    AttemptTerminal,
    RequestTerminal,
}

/// Terminal outcomes for attempt/request completion facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceUsageTerminalOutcomeV1 {
    Succeeded,
    Failed,
    Fallback,
    Cancelled,
    Disconnected,
}

/// Whether token totals were observed from upstream usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceUsageMeasurementCompletenessV1 {
    Unknown,
    UpstreamUsage,
}

/// Request identity snapshot shared by all lifecycle kinds for one request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceUsageRequestEvidenceV1 {
    pub request_id: Uuid,
    pub correlation_id: String,
    pub environment_id: Uuid,
    pub credential_id: Uuid,
    pub credential_generation: u64,
    pub route_id: Uuid,
    pub route_policy_revision: u64,
    pub endpoint: InferenceUsageEndpointV1,
    pub model_alias: String,
    pub model_id: Uuid,
}

impl InferenceUsageRequestEvidenceV1 {
    pub fn validate(&self) -> Result<(), String> {
        validate_uuid("usage request ID", self.request_id)?;
        if self.correlation_id.is_empty() || self.correlation_id.len() > 256 {
            return Err("usage correlation ID is invalid".into());
        }
        validate_uuid("usage environment ID", self.environment_id)?;
        validate_uuid("usage credential ID", self.credential_id)?;
        if self.credential_generation == 0 || self.credential_generation == u64::MAX {
            return Err("usage credential generation is invalid".into());
        }
        validate_uuid("usage route ID", self.route_id)?;
        if self.route_policy_revision == 0 || self.route_policy_revision == u64::MAX {
            return Err("usage route policy revision is invalid".into());
        }
        if self.model_alias.is_empty() || self.model_alias.len() > 256 {
            return Err("usage model alias is invalid".into());
        }
        validate_uuid("usage model ID", self.model_id)?;
        Ok(())
    }
}

/// Attempt identity when a target dispatch is active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceUsageAttemptEvidenceV1 {
    pub attempt_id: Uuid,
    pub target_id: Uuid,
}

impl InferenceUsageAttemptEvidenceV1 {
    pub fn validate(&self) -> Result<(), String> {
        validate_uuid("usage attempt ID", self.attempt_id)?;
        validate_uuid("usage target ID", self.target_id)?;
        Ok(())
    }
}

/// One prompt-free usage lifecycle fact Gateway emits into Cloud.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceUsageLifecycleEventV1 {
    pub schema: String,
    pub kind: InferenceUsageLifecycleKindV1,
    pub occurred_at: DateTime<Utc>,
    pub request: InferenceUsageRequestEvidenceV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<InferenceUsageAttemptEvidenceV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<InferenceUsageTerminalOutcomeV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measurement_completeness: Option<InferenceUsageMeasurementCompletenessV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
}

impl InferenceUsageLifecycleEventV1 {
    pub const SCHEMA: &'static str = INFERENCE_USAGE_LIFECYCLE_SCHEMA_V1;

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported usage lifecycle schema {:?}",
                self.schema
            ));
        }
        self.request.validate()?;
        match self.kind {
            InferenceUsageLifecycleKindV1::RequestStarted => {
                require_none("attempt", self.attempt.as_ref())?;
                require_none("outcome", self.outcome.as_ref())?;
                require_none("http_status", self.http_status.as_ref())?;
                require_none("duration_ms", self.duration_ms.as_ref())?;
                require_none(
                    "measurement_completeness",
                    self.measurement_completeness.as_ref(),
                )?;
                require_none("total_tokens", self.total_tokens.as_ref())?;
            }
            InferenceUsageLifecycleKindV1::AttemptStarted => {
                let attempt = self
                    .attempt
                    .as_ref()
                    .ok_or_else(|| "attempt_started requires attempt evidence".to_string())?;
                attempt.validate()?;
                require_none("outcome", self.outcome.as_ref())?;
                require_none("http_status", self.http_status.as_ref())?;
                require_none("duration_ms", self.duration_ms.as_ref())?;
                require_none(
                    "measurement_completeness",
                    self.measurement_completeness.as_ref(),
                )?;
                require_none("total_tokens", self.total_tokens.as_ref())?;
            }
            InferenceUsageLifecycleKindV1::AttemptTerminal
            | InferenceUsageLifecycleKindV1::RequestTerminal => {
                if matches!(self.kind, InferenceUsageLifecycleKindV1::AttemptTerminal) {
                    let attempt = self
                        .attempt
                        .as_ref()
                        .ok_or_else(|| "attempt_terminal requires attempt evidence".to_string())?;
                    attempt.validate()?;
                } else if let Some(attempt) = &self.attempt {
                    attempt.validate()?;
                }
                if self.outcome.is_none() {
                    return Err("terminal usage lifecycle requires an outcome".into());
                }
                if self.duration_ms.is_none() {
                    return Err("terminal usage lifecycle requires duration_ms".into());
                }
                if self.measurement_completeness.is_none() {
                    return Err("terminal usage lifecycle requires measurement_completeness".into());
                }
                if self.total_tokens == Some(u64::MAX) {
                    return Err("usage total_tokens sentinel is invalid".into());
                }
            }
        }
        Ok(())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        reject_forbidden_keys(bytes)?;
        let event: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("usage lifecycle event is invalid JSON: {error}"))?;
        event.validate()?;
        Ok(event)
    }
}

fn require_none<T>(label: &str, value: Option<&T>) -> Result<(), String> {
    if value.is_some() {
        Err(format!(
            "usage lifecycle {label} must be absent for this kind"
        ))
    } else {
        Ok(())
    }
}

fn validate_uuid(label: &str, value: Uuid) -> Result<(), String> {
    if value.is_nil() {
        Err(format!("{label} is invalid"))
    } else {
        Ok(())
    }
}

fn reject_forbidden_keys(bytes: &[u8]) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("usage lifecycle event is invalid JSON: {error}"))?;
    reject_forbidden_value(&value, "")
}

fn reject_forbidden_value(value: &serde_json::Value, path: &str) -> Result<(), String> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let lowered = key.to_ascii_lowercase();
                if FORBIDDEN_PAYLOAD_KEYS
                    .iter()
                    .any(|forbidden| lowered == *forbidden)
                {
                    return Err(format!(
                        "usage lifecycle forbids prompt or secret field {path}{key}"
                    ));
                }
                let next = if path.is_empty() {
                    format!("{key}.")
                } else {
                    format!("{path}{key}.")
                };
                reject_forbidden_value(child, &next)?;
            }
            Ok(())
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                reject_forbidden_value(child, &format!("{path}{index}."))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> InferenceUsageRequestEvidenceV1 {
        InferenceUsageRequestEvidenceV1 {
            request_id: Uuid::from_u128(1),
            correlation_id: "corr".into(),
            environment_id: Uuid::from_u128(2),
            credential_id: Uuid::from_u128(3),
            credential_generation: 1,
            route_id: Uuid::from_u128(4),
            route_policy_revision: 1,
            endpoint: InferenceUsageEndpointV1::ChatCompletions,
            model_alias: "alias".into(),
            model_id: Uuid::from_u128(5),
        }
    }

    #[test]
    fn request_started_round_trips() {
        let event = InferenceUsageLifecycleEventV1 {
            schema: InferenceUsageLifecycleEventV1::SCHEMA.into(),
            kind: InferenceUsageLifecycleKindV1::RequestStarted,
            occurred_at: DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            request: request(),
            attempt: None,
            outcome: None,
            http_status: None,
            duration_ms: None,
            measurement_completeness: None,
            total_tokens: None,
        };
        let encoded = serde_json::to_vec(&event).unwrap();
        assert_eq!(InferenceUsageLifecycleEventV1::decode(&encoded).unwrap(), event);
    }

    #[test]
    fn prompt_fields_fail_closed() {
        let mut event = serde_json::json!({
            "schema": InferenceUsageLifecycleEventV1::SCHEMA,
            "kind": "request_started",
            "occurred_at": "2026-01-01T00:00:00Z",
            "request": request(),
        });
        event["prompt"] = serde_json::json!("secret text");
        let encoded = serde_json::to_vec(&event).unwrap();
        let err = InferenceUsageLifecycleEventV1::decode(&encoded).unwrap_err();
        assert!(err.contains("forbids"));
    }
}
