#[test]
fn postgres_search_persistence_uses_only_the_typed_a3s_orm_api() {
    let source = include_str!("postgres.rs");
    for forbidden in ["sql_query", "SqlQuery", "sqlx::", "tokio_postgres"] {
        assert!(
            !source.contains(forbidden),
            "search persistence must not contain {forbidden}"
        );
    }
    assert!(source.contains("select_from::<AuthorizedSearchProjections>()"));
}
