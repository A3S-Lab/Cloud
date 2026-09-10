use a3s_orm::orm_table;
use chrono::{DateTime, Utc};
use uuid::Uuid;

orm_table! {
    pub(super) struct InferenceRoutes => "inference_routes" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        router: String => "router",
        policy_revision: u64 => "policy_revision",
        models: serde_json::Value => "models",
        grants: serde_json::Value => "grants",
        domain_claim_id: Uuid => "domain_claim_id",
        gateway_scope_id: Uuid => "gateway_scope_id",
        hostname: String => "hostname",
        path_prefix: String => "path_prefix",
        binding_generation: u64 => "binding_generation",
        aggregate_version: u64 => "aggregate_version",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
        retired_at: Option<DateTime<Utc>> => "retired_at",
    }
}
