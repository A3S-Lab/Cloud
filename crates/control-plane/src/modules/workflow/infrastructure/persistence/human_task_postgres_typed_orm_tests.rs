#[test]
fn postgres_human_task_persistence_limits_raw_sql_to_atomic_lease_claiming() {
    for (name, source) in [
        ("rows.rs", include_str!("human_task_postgres/rows.rs")),
        ("writes.rs", include_str!("human_task_postgres/writes.rs")),
        (
            "writes/resume.rs",
            include_str!("human_task_postgres/writes/resume.rs"),
        ),
    ] {
        for forbidden in ["sql_query", "SqlQuery", "sqlx::", "tokio_postgres"] {
            assert!(
                !source.contains(forbidden),
                "HumanTask {name} must not contain {forbidden}"
            );
        }
    }

    let repository = include_str!("human_task_postgres.rs");
    assert_eq!(
        repository.matches("sql_query::<").count(),
        1,
        "HumanTask persistence permits one reviewed raw SQL escape hatch"
    );
    assert!(repository.contains("sql_query::<ResumeDeliveryClaimRow>"));
    assert!(repository.contains("for update skip locked"));
    assert!(repository.contains("update workflow_resume_outbox as delivery"));
    for forbidden in ["sqlx::", "tokio_postgres"] {
        assert!(!repository.contains(forbidden));
    }

    let schema = include_str!("human_task_postgres/schema.rs");
    for table in [
        "HumanTasks",
        "WorkflowDecisions",
        "WorkflowHumanTaskInbox",
        "WorkflowResumeOutbox",
        "WorkflowResumeReceipts",
    ] {
        assert!(schema.contains(&format!("struct {table}")));
    }
}
