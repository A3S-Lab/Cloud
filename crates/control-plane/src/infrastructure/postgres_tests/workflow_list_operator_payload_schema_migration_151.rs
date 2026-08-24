const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/151_workflow_list_operator_payload_schema.sql"
));

#[test]
fn migration_151_widens_only_the_closed_workflow_payload_schema_registry() {
    let canonical = MIGRATION
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    for expected in [
        "drop constraint workflow_revision_payloads_schema_check",
        "add constraint workflow_revision_payloads_schema_check check",
        "'cloud.workflow.configuration.list-operator.v1'",
        "'cloud.workflow.configuration.v1'",
        "'cloud.workflow.configuration.variable-aggregate.v1'",
        "'cloud.workflow.data-schema.v1'",
        "'cloud.workflow.policy.v1'",
        "'cloud.workflow.policy.v2'",
        "'cloud.workflow.policy.v3'",
        "canonical acl parsing remains the semantic authority",
    ] {
        assert!(canonical.contains(expected), "missing {expected}");
    }

    for forbidden in [
        "create table",
        "add column",
        "create queue",
        "create trigger",
        "update workflow_revision_payloads",
    ] {
        assert!(
            !canonical.contains(forbidden),
            "migration 151 added duplicate state or authority through {forbidden}"
        );
    }
}
