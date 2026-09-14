const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/205_application_feedback_and_annotations.sql"
));

#[test]
fn migration_205_persists_immutable_feedback_and_annotations() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table application_feedbacks",
        "create table application_annotations",
        "rating text not null check (rating in ('positive', 'negative'))",
        "content jsonb not null",
        "reject_application_session_child_mutation",
        "application_feedbacks_session_idx",
        "application_annotations_session_idx",
        "references application_sessions",
        "references application_messages",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 205 is missing {expected}"
        );
    }
    for forbidden in [
        "create table application_annotation_replies",
        "create table application_feedback_cqrs",
        "alter table application_sessions",
        "aggregate_version",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 205 introduced an out-of-scope authority: {forbidden}"
        );
    }
}
