const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/211_gateway_rate_shaping_profiles.sql"
));

#[test]
fn migration_211_persists_upsertable_gateway_rate_shaping_profiles() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table gateway_rate_shaping_profiles",
        "profile_id text primary key",
        "policy_revision_digest text not null",
        "algorithm_kind text not null",
        "token_bucket",
        "gcra",
        "token_bucket_capacity",
        "token_bucket_refill_tokens_per_second",
        "gcra_emission_interval_nanos",
        "gcra_burst_tolerance",
        "updated_at",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 211 is missing {expected}"
        );
    }
    for forbidden in [
        "create table gateway_rate_shaping_profile_cqrs",
        "aggregate_version",
        "create table delivery_rate",
        "redis",
        "path_to_channel",
    ] {
        assert!(
            !lower.contains(&forbidden.to_ascii_lowercase()),
            "migration 211 introduced an out-of-scope authority: {forbidden}"
        );
    }
}
