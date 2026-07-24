use a3s_orm::orm_table;
use chrono::{DateTime, Utc};
use uuid::Uuid;

orm_table! {
    pub(super) struct GatewayRouteScopes => "gateway_route_scopes" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        node_id: Uuid => "node_id",
        membership_generation: u64 => "membership_generation",
        min_ready: u32 => "min_ready",
        max_unavailable: u32 => "max_unavailable",
        aggregate_version: u64 => "aggregate_version",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

orm_table! {
    pub(super) struct GatewayScopeMembers => "gateway_scope_members" {
        gateway_scope_id: Uuid => "gateway_scope_id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        node_id: Uuid => "node_id",
        ordinal: u32 => "ordinal",
        membership_generation: u64 => "membership_generation",
        added_at: DateTime<Utc> => "added_at",
    }
}

orm_table! {
    pub(super) struct GatewayRollouts => "gateway_rollouts" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        gateway_scope_id: Uuid => "gateway_scope_id",
        membership_generation: u64 => "membership_generation",
        generation: u64 => "generation",
        correlation_id: Uuid => "correlation_id",
        min_ready: u32 => "min_ready",
        max_unavailable: u32 => "max_unavailable",
        desired_replicas: u32 => "desired_replicas",
        state: String => "state",
        ready_replicas: u32 => "ready_replicas",
        unavailable_replicas: u32 => "unavailable_replicas",
        aggregate_version: u64 => "aggregate_version",
        started_at: DateTime<Utc> => "started_at",
        completed_at: Option<DateTime<Utc>> => "completed_at",
    }
}

orm_table! {
    pub(super) struct GatewayRolloutReplicas => "gateway_rollout_replicas" {
        gateway_rollout_id: Uuid => "gateway_rollout_id",
        gateway_scope_id: Uuid => "gateway_scope_id",
        membership_generation: u64 => "membership_generation",
        node_id: Uuid => "node_id",
        revision: u64 => "revision",
        command_id: Uuid => "command_id",
        snapshot_digest: String => "snapshot_digest",
        snapshot_expires_at: DateTime<Utc> => "snapshot_expires_at",
        gateway_certificate_id: Option<Uuid> => "gateway_certificate_id",
        state: String => "state",
        failure: Option<String> => "failure",
        acknowledged_at: Option<DateTime<Utc>> => "acknowledged_at",
    }
}

orm_table! {
    pub(super) struct GatewayScopes => "gateway_scopes" {
        node_id: Uuid => "node_id",
        last_issued_revision: u64 => "last_issued_revision",
        installed_revision: Option<u64> => "installed_revision",
        aggregate_version: u64 => "aggregate_version",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

orm_table! {
    pub(super) struct GatewayPublications => "gateway_publications" {
        node_id: Uuid => "node_id",
        revision: u64 => "revision",
        expected_revision: Option<u64> => "expected_revision",
        command_id: Uuid => "command_id",
        command_correlation_id: Uuid => "command_correlation_id",
        snapshot_digest: String => "snapshot_digest",
        acl: String => "acl",
        state: String => "state",
        failure: Option<String> => "failure",
        command_issued_at: DateTime<Utc> => "command_issued_at",
        command_not_after: DateTime<Utc> => "command_not_after",
        snapshot_expires_at: DateTime<Utc> => "snapshot_expires_at",
        acknowledged_at: Option<DateTime<Utc>> => "acknowledged_at",
        certificate_request: Option<serde_json::Value> => "certificate_request",
    }
}
