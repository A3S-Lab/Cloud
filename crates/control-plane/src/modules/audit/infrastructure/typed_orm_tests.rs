#[test]
fn audit_query_reuses_the_shared_records_through_typed_a3s_orm() {
    let source = include_str!("postgres.rs");
    for forbidden in [
        "sql_query",
        "SqlQuery",
        "sqlx::",
        "tokio_postgres",
        "create table",
    ] {
        assert!(
            !source.contains(forbidden),
            "audit query persistence must not contain {forbidden}"
        );
    }
    assert!(source.contains("select_from::<AuditRecords>()"));
    assert!(!source.contains("insert_into"));
}
