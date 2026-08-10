#[test]
fn postgres_operation_persistence_uses_only_typed_a3s_orm_queries() {
    for (name, source) in [
        ("postgres.rs", include_str!("postgres.rs")),
        ("postgres/schema.rs", include_str!("postgres/schema.rs")),
    ] {
        for forbidden in ["sql_query", "SqlQuery", "sqlx::", "tokio_postgres"] {
            assert!(
                !source.contains(forbidden),
                "Operation {name} must not contain {forbidden}"
            );
        }
    }

    let repository = include_str!("postgres.rs");
    for typed_query in [
        "select_from::<OperationRequests>()",
        "select_from::<OperationProjections>()",
        "insert_into::<OperationRequests>()",
        "insert_into::<OperationProjections>()",
    ] {
        assert!(repository.contains(typed_query), "missing {typed_query}");
    }
    assert!(repository.contains("advisory_xact_lock"));
}
