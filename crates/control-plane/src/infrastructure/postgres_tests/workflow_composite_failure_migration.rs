const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/148_workflow_composite_failure_step_projections.sql"
));

#[test]
fn migration_148_admits_only_failed_composite_routing_evidence() {
    for expected in [
        "drop constraint workflow_step_projections_selected_handle_routing_check",
        "add constraint workflow_step_projections_selected_handle_routing_check check",
        "selected_handle is null",
        "kind = 'branch'",
        "kind in ('transform', 'execution', 'service', 'output', 'subworkflow')",
        "status = 'failed'",
        "descriptor-bound",
        "composite-region route",
        "immutable WorkflowRun plan",
    ] {
        assert!(MIGRATION.contains(expected), "missing {expected}");
    }
    for forbidden in ["create table", "add column", "create queue", "retry"] {
        assert!(
            !MIGRATION.to_ascii_lowercase().contains(forbidden),
            "migration 148 added duplicate state or policy: {forbidden}"
        );
    }
}
