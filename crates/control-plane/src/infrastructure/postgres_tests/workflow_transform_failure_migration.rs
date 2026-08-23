const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/145_workflow_transform_failure_step_projections.sql"
));

#[test]
fn migration_145_admits_only_failed_transform_routing_evidence() {
    for expected in [
        "drop constraint workflow_step_projections_selected_handle_routing_check",
        "add constraint workflow_step_projections_selected_handle_routing_check check",
        "selected_handle is null",
        "kind = 'branch'",
        "kind in ('transform', 'execution', 'service', 'output')",
        "status = 'failed'",
        "descriptor-bound Transform",
        "immutable WorkflowRun plan",
    ] {
        assert!(MIGRATION.contains(expected), "missing {expected}");
    }
    for forbidden in ["create table", "add column", "create queue", "retry"] {
        assert!(
            !MIGRATION.to_ascii_lowercase().contains(forbidden),
            "migration 145 added duplicate state or policy: {forbidden}"
        );
    }
}
