const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/152_artifact_build_candidate_projection.sql"
));

#[test]
fn migration_152_creates_one_artifacts_owned_fact_projection_without_foreign_authority() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table artifact_build_candidates",
        "primary key (organization_id, subject_kind, subject_id)",
        "subject_id = source_revision_id",
        "subject_id = asset_release_id",
        "repository_identity text",
        "commit_sha text not null",
        "owner_input_digest text not null",
        "r.recipe_digest",
        "r.manifest_digest",
        "r.updated_at",
        "artifacts-owned immutable projection",
        "not a queue or lifecycle state machine",
        "from external_source_revisions",
        "from asset_releases",
        "r.state = 'draft'",
        "a.state = 'active'",
        "a.kind in ('agent', 'mcp')",
        "drain pre-152 assets writers",
        "asset.hosted-build.requested@1",
    ] {
        assert!(
            lower.contains(expected),
            "migration 152 is missing {expected}"
        );
    }
    for forbidden in [
        "foreign key",
        "references external_source_revisions",
        "references asset_releases",
        "processed_at",
        "claimed_at",
        "lease_owner",
        "retry_count",
    ] {
        assert!(
            !lower.contains(forbidden),
            "candidate projection acquired foreign authority or queue state: {forbidden}"
        );
    }
}
