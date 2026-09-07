const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/189_plugin_assignments.sql"
));

#[test]
fn migration_189_persists_one_live_host_package_assignment() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table plugin_assignments",
        "unique (organization_id, target_host_id, package_id)",
        "desired_state in ('enabled', 'installed-disabled', 'absent')",
        "assignment_generation",
        "selected_surfaces",
        "workspace_scope",
        "references plugin_registries",
        "references nodes",
        "references environments",
        "plugin_assignments_environment_idx",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 189 is missing {expected}"
        );
    }
    for forbidden in [
        "create table plugin_plan_projections",
        "package_bytes",
        "tuf_",
        "capability_registry",
        "install_tree",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 189 introduced out-of-scope storage: {forbidden}"
        );
    }
}
