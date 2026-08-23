#[test]
fn audit_query_and_retention_reuse_shared_authority_through_typed_a3s_orm() {
    let source = include_str!("postgres.rs");
    for forbidden in [
        "AuditRecords::details()",
        "ProjectAttributionProfiles",
        "project_attribution_profiles",
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
    assert!(source.contains("select_from::<AuditRetentionStates>()"));
    assert!(source.contains("delete_from::<AuditRecords>()"));
    assert!(source.contains("update_table::<AuditRetentionStates>()"));
    assert!(source.contains(".for_share()"));
    assert!(source.contains(".for_update()"));
    assert!(source.contains(".skip_locked()"));
    assert!(source.contains(".in_subquery(candidates)"));
    assert!(!source.contains("insert_into"));
}
