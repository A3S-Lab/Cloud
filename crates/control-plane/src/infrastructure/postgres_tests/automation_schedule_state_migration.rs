#[allow(dead_code)]
const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/184_automation_schedule_state.sql"
));

#[test]
fn migration_184_persists_only_exact_schedule_cursor_and_lease_state() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table automation_schedule_states",
        "revision_digest",
        "cursor_at",
        "lease_generation",
        "lease_owner_id",
        "lease_expires_at",
        "enforce_automation_schedule_state_transition",
        "lease_expires_at <= reserved_at + interval '5 minutes'",
        "references environments (organization_id, project_id, id)",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 184 is missing {expected}"
        );
    }
    for forbidden in [
        "create table automation_scheduler",
        "create table automation_schedule_queue",
        "invocation_json",
        "worker_id",
        "retry_count",
        "http_request",
        "secret_material",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 184 introduced an out-of-scope authority: {forbidden}"
        );
    }
}
