#[test]
fn postgres_asset_persistence_uses_only_the_typed_a3s_orm_boundary() {
    let source = [
        include_str!("mod.rs"),
        include_str!("git_controls.rs"),
        include_str!("hosted_publications.rs"),
        include_str!("mcp_profiles.rs"),
        include_str!("queries.rs"),
        include_str!("rows.rs"),
        include_str!("writes.rs"),
    ]
    .join("\n");
    assert!(source.contains("a3s_orm"));
    for forbidden in [
        "tokio_postgres",
        "deadpool_postgres",
        "sqlx::",
        ".pool()",
        "batch_execute(",
    ] {
        assert!(
            !source.contains(forbidden),
            "Asset persistence must use A3S ORM; found {forbidden}"
        );
    }
}
