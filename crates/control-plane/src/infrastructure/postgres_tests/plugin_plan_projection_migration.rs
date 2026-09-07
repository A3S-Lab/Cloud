const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/190_plugin_plan_projections.sql"
));

#[test]
fn migration_190_persists_bounded_plan_review_digests_only() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table plugin_plan_projections",
        "unique (organization_id, operation_id, plan_digest)",
        "plan_schema = 'a3s.use.plugin-operation-plan.v4'",
        "authority_decision in ('allow', 'ask', 'deny')",
        "confirmation_digest",
        "impact_digest",
        "permission_evidence_digest",
        "provider_evidence_digest",
        "references plugin_assignments",
        "plugin_plan_projections_assignment_idx",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 190 is missing {expected}"
        );
    }
    for forbidden in [
        "package_bytes",
        "package_lock",
        "secret_value",
        "tuf_",
        "capability_registry",
        "install_tree",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 190 introduced out-of-scope storage: {forbidden}"
        );
    }
}
