use a3s_orm::orm_table;
use chrono::{DateTime, Utc};
use uuid::Uuid;

orm_table! {
    pub(crate) struct MigrationRecords => "a3s_orm_migrations" {
        version: String => "version",
        checksum: String => "checksum",
    }
}

orm_table! {
    pub(crate) struct InfrastructureBindings => "infrastructure_bindings" {
        binding_name: String => "binding_name",
        binding_schema: String => "binding_schema",
        binding_digest: String => "binding_digest",
        bound_at: DateTime<Utc> => "bound_at",
    }
}

orm_table! {
    pub(crate) struct IdempotencyRecords => "idempotency_records" {
        scope_key: String => "scope_key",
        idempotency_key: String => "idempotency_key",
        request_digest: String => "request_digest",
        response: serde_json::Value => "response",
        created_at: DateTime<Utc> => "created_at",
    }
}

orm_table! {
    pub(crate) struct OutboxEvents => "outbox_events" {
        event_id: Uuid => "event_id",
        event_key: String => "event_key",
        schema_version: u32 => "schema_version",
        organization_id: Uuid => "organization_id",
        aggregate_id: Uuid => "aggregate_id",
        aggregate_version: u64 => "aggregate_version",
        occurred_at: DateTime<Utc> => "occurred_at",
        correlation_id: Uuid => "correlation_id",
        causation_id: Option<Uuid> => "causation_id",
        payload: serde_json::Value => "payload",
        published_at: Option<DateTime<Utc>> => "published_at",
        delivery_attempts: u32 => "delivery_attempts",
        last_error: Option<String> => "last_error",
    }
}

orm_table! {
    pub(crate) struct AuditRecords => "audit_records" {
        audit_id: Uuid => "audit_id",
        organization_id: Uuid => "organization_id",
        actor_id: Option<Uuid> => "actor_id",
        action: String => "action",
        aggregate_id: Uuid => "aggregate_id",
        occurred_at: DateTime<Utc> => "occurred_at",
        request_id: Uuid => "request_id",
        project_id: Option<Uuid> => "project_id",
        environment_id: Option<Uuid> => "environment_id",
        attribution_profile_id: Option<Uuid> => "attribution_profile_id",
        attribution_status: String => "attribution_status",
        details: serde_json::Value => "details",
    }
}
