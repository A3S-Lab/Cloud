const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/188_automation_definitions_and_revisions.sql"
));

#[test]
fn migration_188_persists_an_immutable_definition_head_and_revision_chain() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table automation_definitions",
        "create table automation_revisions",
        "current_revision_id",
        "revision_acl",
        "foreign key (organization_id, parent_revision_id)",
        "automation_definitions_validate_head",
        "enforce_automation_revision_lineage",
        "automation_revisions_immutable",
        "deferrable initially deferred",
        "not own timers, queues, target execution",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 188 is missing {expected}"
        );
    }
    for forbidden in [
        "create table automation_scheduler",
        "retry_count",
        "target_url",
        "secret_material",
        "queue_state",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 188 introduced an out-of-scope authority: {forbidden}"
        );
    }
}
