const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/210_application_publication_route_intents.sql"
));

#[test]
fn migration_210_persists_immutable_publication_route_intents() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table application_publication_route_intents",
        "channels text[] not null",
        "embed_origin_allowlist text[] not null default '{}'",
        "rate_shaping_profile_id text not null",
        "rate_shaping_policy_revision_digest text not null",
        "api_blocking",
        "api_streaming",
        "cardinality(channels) >= 1",
        "references application_releases",
        "contract_digest",
        "application_publication_route_intents_release_idx",
        "reject_application_publication_route_intent_mutation",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 210 is missing {expected}"
        );
    }
    for forbidden in [
        "created_at",
        "create table application_publication_route_intent_cqrs",
        "aggregate_version",
        "alter table application_releases",
        "create table application_publication_route_commands",
    ] {
        assert!(
            !lower.contains(&forbidden.to_ascii_lowercase()),
            "migration 210 introduced an out-of-scope authority: {forbidden}"
        );
    }
}