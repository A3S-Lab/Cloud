#[test]
fn postgres_human_task_persistence_uses_only_typed_a3s_orm_queries() {
    for (name, source) in [
        (
            "human_task_postgres.rs",
            include_str!("human_task_postgres.rs"),
        ),
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
    assert!(repository.contains("select_from::<WorkflowResumeOutbox>()"));
    assert!(repository.contains(".for_update_of::<WorkflowResumeOutbox>()"));
    assert!(repository.contains(".skip_locked()"));
    assert!(repository.contains("update_table::<WorkflowResumeOutbox>()"));
    assert!(repository.contains(".from::<WorkflowResumeCandidates>()"));
    assert!(repository.contains(".returning(ResumeDeliveryClaimSelection)"));

    let schema = include_str!("human_task_postgres/schema.rs");
    for table in [
        "HumanTasks",
        "WorkflowDecisions",
        "WorkflowHumanTaskInbox",
        "WorkflowResumeOutbox",
        "WorkflowResumeCandidates",
        "WorkflowResumeReceipts",
    ] {
        assert!(schema.contains(&format!("struct {table}")));
    }
}
