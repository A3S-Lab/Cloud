use crate::modules::inference::domain::value_objects::EdgeRouteBindingRef;
use crate::modules::inference::domain::{
    InferenceRoute, InferenceUsageDailyRollup, InferenceUsageRequestFact,
    InferenceUsageRetentionStatus,
};
use crate::modules::inference::InferenceRoutePage;
use a3s_cloud_contracts::{
    InferenceEndpointAcl, InferenceGrantAclProjection, InferenceLimitsAclProjection,
    InferenceModelAclProjection, InferenceTargetAclProjection, InferenceUsageEndpointV1,
    InferenceUsageMeasurementCompletenessV1, InferenceUsageTerminalOutcomeV1,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceUsageRetentionStatusResponse {
    pub organization_id: Uuid,
    pub retention_ms: u64,
    pub policy_digest: String,
    pub applied_policy_digest: Option<String>,
    pub current_policy_applied: bool,
    pub records_available_from: Option<DateTime<Utc>>,
    pub records_deleted_before: Option<DateTime<Utc>>,
    pub total_deleted_records: u64,
    pub last_swept_at: Option<DateTime<Utc>>,
    pub last_completed_at: Option<DateTime<Utc>>,
    pub next_scan_at: DateTime<Utc>,
    pub version: u64,
}

impl From<InferenceUsageRetentionStatus> for InferenceUsageRetentionStatusResponse {
    fn from(status: InferenceUsageRetentionStatus) -> Self {
        Self {
            organization_id: status.organization_id.as_uuid(),
            retention_ms: status.retention_ms,
            policy_digest: status.policy_digest.to_string(),
            applied_policy_digest: status
                .applied_policy_digest
                .map(|digest| digest.to_string()),
            current_policy_applied: status.current_policy_applied,
            records_available_from: status.records_available_from,
            records_deleted_before: status.records_deleted_before,
            total_deleted_records: status.total_deleted_records,
            last_swept_at: status.last_swept_at,
            last_completed_at: status.last_completed_at,
            next_scan_at: status.next_scan_at,
            version: status.version,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceRouteResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub router: String,
    pub policy_revision: u64,
    pub models: Vec<InferenceModelResponse>,
    pub grants: Vec<InferenceGrantResponse>,
    pub binding: EdgeRouteBindingResponse,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub retired_at: Option<DateTime<Utc>>,
}

impl From<InferenceRoute> for InferenceRouteResponse {
    fn from(route: InferenceRoute) -> Self {
        Self {
            id: route.id.as_uuid(),
            organization_id: route.organization_id.as_uuid(),
            project_id: route.project_id.as_uuid(),
            environment_id: route.environment_id.as_uuid(),
            router: route.router().into(),
            policy_revision: route.policy_revision(),
            models: route.models().iter().cloned().map(Into::into).collect(),
            grants: route.grants().iter().cloned().map(Into::into).collect(),
            binding: route.binding().clone().into(),
            aggregate_version: route.aggregate_version(),
            created_at: route.created_at(),
            updated_at: route.updated_at(),
            retired_at: route.retired_at(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceRoutePageResponse {
    pub items: Vec<InferenceRouteResponse>,
    pub next_cursor: Option<String>,
}

impl From<InferenceRoutePage> for InferenceRoutePageResponse {
    fn from(page: InferenceRoutePage) -> Self {
        Self {
            items: page.routes.into_iter().map(Into::into).collect(),
            next_cursor: page.next_cursor,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceModelResponse {
    pub alias: String,
    pub model_id: Uuid,
    pub targets: Vec<InferenceTargetResponse>,
}

impl From<InferenceModelAclProjection> for InferenceModelResponse {
    fn from(model: InferenceModelAclProjection) -> Self {
        Self {
            alias: model.alias,
            model_id: model.model_id,
            targets: model.targets.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceTargetResponse {
    pub target_id: Uuid,
    pub service: String,
    pub upstream_model: String,
    pub priority: u32,
    pub weight: u32,
}

impl From<InferenceTargetAclProjection> for InferenceTargetResponse {
    fn from(target: InferenceTargetAclProjection) -> Self {
        Self {
            target_id: target.target_id,
            service: target.service,
            upstream_model: target.upstream_model,
            priority: target.priority,
            weight: target.weight,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceGrantResponse {
    pub credential_id: Uuid,
    pub credential_generation: u64,
    pub models: Vec<String>,
    pub endpoints: Vec<InferenceEndpointAcl>,
    pub limits: InferenceLimitsResponse,
}

impl From<InferenceGrantAclProjection> for InferenceGrantResponse {
    fn from(grant: InferenceGrantAclProjection) -> Self {
        Self {
            credential_id: grant.credential_id,
            credential_generation: grant.credential_generation,
            models: grant.models,
            endpoints: grant.endpoints,
            limits: grant.limits.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceLimitsResponse {
    pub max_concurrent_requests: u64,
    pub requests_per_minute: u64,
    pub request_burst: u64,
    pub tokens_per_minute: u64,
}

impl From<InferenceLimitsAclProjection> for InferenceLimitsResponse {
    fn from(limits: InferenceLimitsAclProjection) -> Self {
        Self {
            max_concurrent_requests: limits.max_concurrent_requests,
            requests_per_minute: limits.requests_per_minute,
            request_burst: limits.request_burst,
            tokens_per_minute: limits.tokens_per_minute,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeRouteBindingResponse {
    pub domain_claim_id: Uuid,
    pub gateway_scope_id: Uuid,
    pub hostname: String,
    pub path_prefix: String,
    pub binding_generation: u64,
}

impl From<EdgeRouteBindingRef> for EdgeRouteBindingResponse {
    fn from(binding: EdgeRouteBindingRef) -> Self {
        Self {
            domain_claim_id: binding.domain_claim_id.as_uuid(),
            gateway_scope_id: binding.gateway_scope_id.as_uuid(),
            hostname: binding.hostname,
            path_prefix: binding.path_prefix,
            binding_generation: binding.binding_generation,
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
