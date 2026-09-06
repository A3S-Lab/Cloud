const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/186_automation_schedule_lease_release.sql"
));

#[test]
fn migration_186_allows_only_fenced_equal_cursor_release() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create or replace function enforce_automation_schedule_state_transition",
        "old.lease_owner_id is not null and new.lease_owner_id is null",
        "new.cursor_at < old.cursor_at",
        "new.updated_at < old.reserved_at",
        "lease release or commit is not fenced",
    ] {
        assert!(
            lower.contains(expected),
            "migration 186 is missing {expected}"
        );
    }
    for forbidden in [
        "create table automation_schedule_queue",
        "create table automation_scheduler",
        "retry_count",
        "worker_id",
        "invocation_json",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 186 introduced an out-of-scope authority: {forbidden}"
        );
    }
}
