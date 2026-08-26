const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/162_artifact_preview_build_lifecycle_projections.sql"
));

#[test]
fn migration_162_adds_one_artifacts_owned_preview_fence_without_another_build_lifecycle() {
    let lower = MIGRATION.to_ascii_lowercase();
    for required in [
        "alter table artifact_build_candidates",
        "add column preview_id uuid",
        "create table artifact_preview_build_lifecycle_projections",
        "preview_aggregate_version desc",
        "artifact_preview_build_lifecycle_retry_authority_idx",
        "artifact_build_candidates_immutable",
        "artifact build candidate fact projections are immutable",
        "artifact_preview_build_lifecycle_projections_immutable",
        "preview build lifecycle projection receipts are immutable",
        "source.pull-request-preview-revision.lifecycle-committed",
        "every applied preview sourcerevision receipt must have one exact specialized lifecycle fact",
        "pre-upgrade preview sourcerevision already has a buildrun without lifecycle retirement authority",
        "retired_build_run_id uuid",
        "references build_runs",
        "not an inbox, queue, worker, saga, scheduler, or second buildrun lifecycle",
        "delivery/retry remain on the existing outbox relay",
        "octet_length(e.payload::text) > 16384",
        "e.payload ?& array",
        "null::uuid",
    ] {
        assert!(
            lower.contains(required),
            "migration 162 lost required invariant {required}"
        );
    }
    assert_eq!(
        lower.matches("null::uuid").count(),
        2,
        "Preview candidate backfill must type both absent Asset UUIDs"
    );
    for forbidden in [
        "create table artifact_preview_build_lifecycle_heads",
        "create table artifact_preview_build_lifecycle_inbox",
        "create table artifact_preview_build_lifecycle_queue",
        "create table artifact_preview_build_lifecycle_jobs",
        "processed_at",
        "claimed_at",
        "lease_owner",
        "retry_count",
        "create extension pgcrypto",
        "digest(",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 162 introduced duplicate delivery/build authority {forbidden}"
        );
    }
}
