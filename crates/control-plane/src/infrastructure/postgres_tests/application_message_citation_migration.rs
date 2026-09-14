const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/208_application_message_citations.sql"
));

#[test]
fn migration_208_persists_immutable_message_citations() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table application_message_citations",
        "message_kind text not null check",
        "message_kind in ('answer', 'final_output')",
        "reject_application_session_child_mutation",
        "application_message_citations_session_idx",
        "references application_sessions",
        "references application_messages",
        "references application_invocations",
        "references knowledge_bases",
        "references knowledge_base_revisions",
        "references knowledge_documents",
        "references knowledge_chunks",
        "excerpt_digest text not null",
        "knowledge_chunk_id uuid not null",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 208 is missing {expected}"
        );
    }
    for forbidden in [
        "create table application_message_citation_cqrs",
        "alter table application_sessions",
        "aggregate_version",
        "create table application_citations",
        "create table application_message_file_references",
        "create table application_message_variants",
        "references user_files",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 208 introduced an out-of-scope authority: {forbidden}"
        );
    }
}

