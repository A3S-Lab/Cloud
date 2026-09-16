const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/209_resource_grant_application_scope.sql"
));

#[test]
fn migration_209_admits_exact_application_resource_grant_scope() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "application_id",
        "'application'",
        "resource_grants_active_application_idx",
        "references applications (organization_id, project_id, id)",
        "scope_kind = 'application'",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 209 is missing {expected}"
        );
    }
    for forbidden in [
        "gateway",
        " sse",
        "browser",
        "process_role",
        "delivery_role",
    ] {
        assert!(
            !lower.contains(forbidden.trim()),
            "migration 209 introduced an out-of-scope authority: {forbidden}"
        );
    }
}
