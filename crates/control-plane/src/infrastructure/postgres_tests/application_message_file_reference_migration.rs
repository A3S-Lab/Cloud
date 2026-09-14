const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/207_application_message_file_references.sql"
));

#[test]
fn migration_207_persists_immutable_message_file_references() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table application_message_file_references",
        "message_kind text not null check",
        "message_kind in ('input')",
        "reject_application_session_child_mutation",
        "application_message_file_references_session_idx",
        "references application_sessions",
        "references application_messages",
        "references application_invocations",
        "references user_files",
        "content_digest text not null",
        "user_file_id uuid not null",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 207 is missing {expected}"
        );
    }
    for forbidden in [
        "create table application_message_file_reference_cqrs",
        "alter table application_sessions",
        "aggregate_version",
        "create table application_citations",
        "create table application_message_variants",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 207 introduced an out-of-scope authority: {forbidden}"
        );
    }
}
