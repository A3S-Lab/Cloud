const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/187_automation_invocation_admission.sql"
));

#[test]
fn migration_187_keeps_exact_invocation_admission_immutable_and_scoped() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table automation_invocations",
        "unique (organization_id, automation_id, deduplication_key)",
        "invocation_digest",
        "invocation_json",
        "automation_invocations_immutable",
        "enforce_automation_invocation_immutability",
        "foreign key (organization_id, project_id, environment_id)",
        "not a scheduler, queue, worker, or target store",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 187 is missing {expected}"
        );
    }
    for forbidden in [
        "retry_count",
        "next_attempt",
        "target_url",
        "secret_material",
        "create table automation_scheduler",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 187 introduced an out-of-scope authority: {forbidden}"
        );
    }
}
