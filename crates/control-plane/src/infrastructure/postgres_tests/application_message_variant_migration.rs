const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/206_application_message_variants.sql"
));

#[test]
fn migration_206_persists_immutable_message_variants() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table application_message_variants",
        "source_message_kind text not null check",
        "source_message_kind in ('answer', 'final_output')",
        "reject_application_session_child_mutation",
        "application_message_variants_session_idx",
        "references application_sessions",
        "references application_messages",
        "references application_invocations",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 206 is missing {expected}"
        );
    }
    for forbidden in [
        "create table application_message_variant_cqrs",
        "alter table application_sessions",
        "aggregate_version",
        "create table application_message_regenerations",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 206 introduced an out-of-scope authority: {forbidden}"
        );
    }
}
