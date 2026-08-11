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

#[test]
fn plugin_registries_extend_the_existing_authorized_search_view() {
    let migration =
        include_str!("../../../../../../../migrations/085_plugin_registry_search_projection.sql");
    assert_eq!(
        migration
            .matches("create view authorized_search_projections")
            .count(),
        1
    );
    assert!(migration.contains("'plugin_registry'::text"));
    assert!(migration.contains("from plugin_registries as registry"));
    assert!(!migration.contains("create table"));
    assert!(!migration.contains("create materialized view"));
}
