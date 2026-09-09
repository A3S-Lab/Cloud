use crate::modules::inference::domain::{InferenceUsageDailyRollup, InferenceUsageRequestFact};
use a3s_cloud_contracts::{
    InferenceUsageEndpointV1, InferenceUsageMeasurementCompletenessV1,
    InferenceUsageTerminalOutcomeV1,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsageRollupResponse {
    pub day: NaiveDate,
    pub environment_id: Uuid,
    pub model_id: Uuid,
    pub endpoint: String,
    pub request_count: u64,
    pub succeeded_count: u64,
    pub failed_count: u64,
    pub fallback_count: u64,
    pub cancelled_count: u64,
    pub disconnected_count: u64,
    pub unknown_measurement_count: u64,
    pub upstream_usage_count: u64,
    pub total_tokens: u64,
}

impl From<InferenceUsageDailyRollup> for DailyUsageRollupResponse {
    fn from(value: InferenceUsageDailyRollup) -> Self {
        Self {
            day: value.key.day,
            environment_id: value.key.environment_id,
            model_id: value.key.model_id,
            endpoint: endpoint_str(value.key.endpoint).into(),
            request_count: value.request_count,
            succeeded_count: value.succeeded_count,
            failed_count: value.failed_count,
            fallback_count: value.fallback_count,
            cancelled_count: value.cancelled_count,
            disconnected_count: value.disconnected_count,
            unknown_measurement_count: value.unknown_measurement_count,
            upstream_usage_count: value.upstream_usage_count,
            total_tokens: value.total_tokens,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRequestFactResponse {
    pub request_id: Uuid,
    pub gateway_id: Uuid,
    pub environment_id: Uuid,
    pub credential_id: Uuid,
    pub credential_generation: u64,
    pub route_id: Uuid,
    pub route_policy_revision: u64,
    pub endpoint: String,
    pub model_alias: String,
    pub model_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub terminated_at: Option<DateTime<Utc>>,
    pub outcome: Option<String>,
    pub http_status: Option<u16>,
    pub duration_ms: Option<u64>,
    pub measurement_completeness: Option<String>,
    pub total_tokens: Option<u64>,
    pub attempt_count: u32,
}

impl From<InferenceUsageRequestFact> for UsageRequestFactResponse {
    fn from(value: InferenceUsageRequestFact) -> Self {
        Self {
            request_id: value.request_id,
            gateway_id: value.gateway_id,
            environment_id: value.environment_id,
            credential_id: value.credential_id,
            credential_generation: value.credential_generation,
            route_id: value.route_id,
            route_policy_revision: value.route_policy_revision,
            endpoint: endpoint_str(value.endpoint).into(),
            model_alias: value.model_alias,
            model_id: value.model_id,
            started_at: value.started_at,
            terminated_at: value.terminated_at,
            outcome: value.outcome.map(outcome_str).map(str::to_owned),
            http_status: value.http_status,
            duration_ms: value.duration_ms,
            measurement_completeness: value
                .measurement_completeness
                .map(measurement_str)
                .map(str::to_owned),
            total_tokens: value.total_tokens,
            attempt_count: value.attempt_count,
        }
    }
}

fn endpoint_str(endpoint: InferenceUsageEndpointV1) -> &'static str {
    match endpoint {
        InferenceUsageEndpointV1::Models => "models",
        InferenceUsageEndpointV1::ChatCompletions => "chat-completions",
        InferenceUsageEndpointV1::Completions => "completions",
        InferenceUsageEndpointV1::Embeddings => "embeddings",
    }
}

fn outcome_str(outcome: InferenceUsageTerminalOutcomeV1) -> &'static str {
    match outcome {
        InferenceUsageTerminalOutcomeV1::Succeeded => "succeeded",
        InferenceUsageTerminalOutcomeV1::Failed => "failed",
        InferenceUsageTerminalOutcomeV1::Fallback => "fallback",
        InferenceUsageTerminalOutcomeV1::Cancelled => "cancelled",
        InferenceUsageTerminalOutcomeV1::Disconnected => "disconnected",
    }
}

fn measurement_str(value: InferenceUsageMeasurementCompletenessV1) -> &'static str {
    match value {
        InferenceUsageMeasurementCompletenessV1::Unknown => "unknown",
        InferenceUsageMeasurementCompletenessV1::UpstreamUsage => "upstream_usage",
    }
}
