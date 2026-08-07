use crate::modules::assets::domain::{
    McpServiceProfileBinding, McpServiceProfileSpec, McpServiceProfileWrite,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServiceProfileSpecResponse {
    pub protocol_versions: Vec<String>,
    pub endpoint_path: String,
    pub runtime_port: String,
    pub health_path: String,
    pub request_sse: bool,
    pub subscriptions: bool,
    pub server_discover: bool,
    pub expected_capabilities: Vec<String>,
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
    pub max_stream_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServiceProfileResponse {
    pub organization_id: Uuid,
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub profile_digest: String,
    pub acl: String,
    pub spec: McpServiceProfileSpecResponse,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replayed: Option<bool>,
}

impl From<McpServiceProfileSpec> for McpServiceProfileSpecResponse {
    fn from(spec: McpServiceProfileSpec) -> Self {
        Self {
            protocol_versions: spec.protocol_versions,
            endpoint_path: spec.endpoint_path,
            runtime_port: spec.runtime_port,
            health_path: spec.health_path,
            request_sse: spec.request_sse,
            subscriptions: spec.subscriptions,
            server_discover: spec.server_discover,
            expected_capabilities: spec.expected_capabilities,
            max_request_bytes: spec.max_request_bytes,
            max_response_bytes: spec.max_response_bytes,
            max_stream_seconds: spec.max_stream_seconds,
        }
    }
}

impl From<McpServiceProfileBinding> for McpServiceProfileResponse {
    fn from(binding: McpServiceProfileBinding) -> Self {
        Self {
            organization_id: binding.organization_id.as_uuid(),
            asset_id: binding.asset_id.as_uuid(),
            asset_release_id: binding.asset_release_id.as_uuid(),
            profile_digest: binding.profile.digest().to_string(),
            acl: binding.profile.canonical_acl().to_owned(),
            spec: binding.profile.spec().clone().into(),
            created_at: binding.created_at,
            replayed: None,
        }
    }
}

impl From<McpServiceProfileWrite> for McpServiceProfileResponse {
    fn from(write: McpServiceProfileWrite) -> Self {
        let mut response = Self::from(write.binding);
        response.replayed = Some(write.replayed);
        response
    }
}
