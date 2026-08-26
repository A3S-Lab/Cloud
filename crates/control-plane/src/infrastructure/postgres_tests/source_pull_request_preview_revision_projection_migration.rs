const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/159_source_pull_request_preview_revision_projections.sql"
));

#[test]
fn migration_159_uses_one_append_only_sources_fence_without_another_delivery_mechanism() {
    for required in [
        "create table source_pull_request_preview_revision_projections",
        "preview_aggregate_version desc",
        "source_pull_request_preview_revision_projections_immutable",
        "Preview Source revision projection receipts are immutable",
        "references github_repository_subscriptions",
        "references external_source_revisions",
        "its existing Environment foreign key proves the Projects handoff",
        "not another Inbox, queue, worker, saga, or scheduler",
    ] {
        assert!(
            MIGRATION.contains(required),
            "migration 159 lost required invariant {required}"
        );
    }
    let lower = MIGRATION.to_ascii_lowercase();
    for forbidden in [
        "create table source_pull_request_preview_revision_heads",
        "create table source_pull_request_preview_revision_inbox",
        "create table source_pull_request_preview_revision_queue",
        "create table source_pull_request_preview_revision_jobs",
        "retry_count",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 159 introduced duplicate state or delivery authority {forbidden}"
        );
    }
}
