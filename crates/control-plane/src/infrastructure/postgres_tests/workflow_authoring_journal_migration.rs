#[allow(dead_code)]
const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/185_workflow_authoring_journal.sql"
));

#[test]
fn migration_185_persists_bounded_opaque_authoring_state() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table workflow_authoring_journals",
        "create table workflow_authoring_entries",
        "initial_snapshot_bytes",
        "current_snapshot_bytes",
        "next_sequence",
        "unique (organization_id, project_id, workflow_definition_id, operation_id)",
        "references workflow_definitions (organization_id, project_id, id)",
        "for each row execute function enforce_workflow_authoring_journal_transition",
        "for each row execute function reject_workflow_authoring_entry_mutation",
        "octet_length(operation_bytes) between 1 and 1048576",
        "octet_length(current_snapshot_bytes) between 1 and 8388608",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 185 is missing {expected}"
        );
    }
    for forbidden in [
        "create table flow_events",
        "create table workflow_execution_history",
        "create table workflow_task_queue",
        "retry_count",
        "lease_owner_id",
        "secret_material",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 185 introduced an out-of-scope authority: {forbidden}"
        );
    }
}
