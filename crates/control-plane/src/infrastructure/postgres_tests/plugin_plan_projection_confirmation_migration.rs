const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/191_plugin_plan_projection_confirmation.sql"
));

#[test]
fn migration_191_retains_digest_bound_confirmation_envelope() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "add column confirmation jsonb",
        "plugin_plan_projections_confirmation_pair_chk",
        "confirmation_digest is null and confirmation is null",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 191 is missing {expected}"
        );
    }
    for forbidden in ["package_bytes", "package_lock", "secret_value", "tuf_"] {
        assert!(
            !lower.contains(forbidden),
            "migration 191 introduced out-of-scope storage: {forbidden}"
        );
    }
}
