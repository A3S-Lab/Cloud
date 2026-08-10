#[test]
fn postgres_workflow_run_persistence_uses_only_typed_a3s_orm_queries() {
    for (name, source) in [
        (
            "workflow_run_postgres.rs",
            include_str!("workflow_run_postgres.rs"),
        ),
        (
            "workflow_run_postgres/rows.rs",
            include_str!("workflow_run_postgres/rows.rs"),
        ),
        (
            "workflow_run_postgres/schema.rs",
            include_str!("workflow_run_postgres/schema.rs"),
        ),
    ] {
        for forbidden in ["sql_query", "SqlQuery", "sqlx::", "tokio_postgres"] {
            assert!(
                !source.contains(forbidden),
                "WorkflowRun {name} must not contain {forbidden}"
            );
        }
    }

    let repository = include_str!("workflow_run_postgres.rs");
    for typed_query in [
        "select_from::<WorkflowRuns>()",
        "select_from::<WorkflowStepProjections>()",
        "insert_into::<OperationRequests>()",
        "insert_into::<WorkflowRuns>()",
        "insert_into::<WorkflowStepProjections>()",
        "update_table::<WorkflowRuns>()",
        "update_table::<WorkflowStepProjections>()",
    ] {
        assert!(repository.contains(typed_query), "missing {typed_query}");
    }
}
