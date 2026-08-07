use crate::modules::edge::domain::repositories::McpRoutePolicyWrite;
use crate::modules::edge::domain::McpRoutePolicy;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpRoutePolicyLimitResponse {
    pub max_concurrent_requests: u64,
    pub requests_per_minute: u64,
    pub request_burst: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpRoutePolicyGrantResponse {
    pub credential_id: Uuid,
    pub credential_generation: u64,
    pub methods: Vec<String>,
    pub names: Vec<String>,
    pub limits: McpRoutePolicyLimitResponse,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpRoutePolicyResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub gateway_scope_id: Uuid,
    pub domain_claim_id: Uuid,
    pub workload_id: Uuid,
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub profile_digest: String,
    pub hostname: String,
    pub path: String,
    pub tls_required: bool,
    pub allowed_origins: Vec<String>,
    pub max_header_bytes: u64,
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
    pub first_response_timeout_seconds: u64,
    pub stream_idle_timeout_seconds: u64,
    pub stream_total_timeout_seconds: u64,
    pub drain_timeout_seconds: u64,
    pub telemetry_names: Vec<String>,
    pub telemetry_events_per_minute: u64,
    pub audit_required: bool,
    pub grants: Vec<McpRoutePolicyGrantResponse>,
    pub policy_revision: u64,
    pub policy_digest: String,
    pub acl: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<McpRoutePolicy> for McpRoutePolicyResponse {
    fn from(policy: McpRoutePolicy) -> Self {
        let spec = policy.spec();
        Self {
            id: spec.route_id.as_uuid(),
            organization_id: spec.organization_id.as_uuid(),
            project_id: spec.project_id.as_uuid(),
            environment_id: spec.environment_id.as_uuid(),
            gateway_scope_id: spec.gateway_scope_id.as_uuid(),
            domain_claim_id: spec.domain_claim_id.as_uuid(),
            workload_id: spec.workload_id.as_uuid(),
            asset_id: spec.asset_id.as_uuid(),
            asset_release_id: spec.asset_release_id.as_uuid(),
            profile_digest: spec.profile_digest.to_string(),
            hostname: spec.hostname.as_str().into(),
            path: spec.path.clone(),
            tls_required: spec.tls_required,
            allowed_origins: spec.allowed_origins.clone(),
            max_header_bytes: spec.max_header_bytes,
            max_request_bytes: spec.max_request_bytes,
            max_response_bytes: spec.max_response_bytes,
            first_response_timeout_seconds: spec.first_response_timeout_seconds,
            stream_idle_timeout_seconds: spec.stream_idle_timeout_seconds,
            stream_total_timeout_seconds: spec.stream_total_timeout_seconds,
            drain_timeout_seconds: spec.drain_timeout_seconds,
            telemetry_names: spec.telemetry_names.clone(),
            telemetry_events_per_minute: spec.telemetry_events_per_minute,
            audit_required: spec.audit_required,
            grants: spec
                .grants
                .iter()
                .map(|grant| McpRoutePolicyGrantResponse {
                    credential_id: grant.credential_id,
                    credential_generation: grant.credential_generation,
                    methods: grant.methods.clone(),
                    names: grant.names.clone(),
                    limits: McpRoutePolicyLimitResponse {
                        max_concurrent_requests: grant.limits.max_concurrent_requests,
                        requests_per_minute: grant.limits.requests_per_minute,
                        request_burst: grant.limits.request_burst,
                    },
                })
                .collect(),
            policy_revision: policy.policy_revision(),
            policy_digest: policy.policy_digest().to_string(),
            acl: policy.canonical_acl().into(),
            expires_at: spec.expires_at,
            created_at: policy.created_at(),
            updated_at: policy.updated_at(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpRoutePolicyMutationResponse {
    pub policy: McpRoutePolicyResponse,
    pub replayed: bool,
}

impl From<McpRoutePolicyWrite> for McpRoutePolicyMutationResponse {
    fn from(write: McpRoutePolicyWrite) -> Self {
        Self {
            policy: write.policy.into(),
            replayed: write.replayed,
        }
    }
}
