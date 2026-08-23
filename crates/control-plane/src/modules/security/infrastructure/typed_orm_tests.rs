#[test]
fn security_timeline_reuses_typed_owner_facts_without_reading_private_audit_details() {
    let source = include_str!("postgres.rs");
    for forbidden in [
        "sql_query",
        "SqlQuery",
        "sqlx::",
        "tokio_postgres",
        "AuditRecords::details",
        "insert_into",
        "update_table",
        "delete_from",
    ] {
        assert!(
            !source.contains(forbidden),
            "security timeline persistence must not contain {forbidden}"
        );
    }
    assert!(source.contains("select_from::<OutboxEvents>()"));
    assert!(source.contains("left_join::<AuditRecords>"));
    assert!(source.contains("jsonb_build_object"));
    assert!(source.contains("cast::<String, String>(bound::<String>(key), \"text\")"));
}
