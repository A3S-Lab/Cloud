const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/150_hosted_build_context_boundary.sql"
));

#[test]
fn migration_150_documents_identity_guards_without_cross_context_write_authority() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "comment on column asset_releases.build_run_id",
        "comment on constraint asset_releases_hosted_build_foreign_key",
        "comment on constraint build_runs_asset_release_foreign_key",
        "comment on constraint build_runs_hosted_release_publication_identity_unique",
        "relational identity guard",
        "no artifacts write authority over assets",
        "assets-owned",
    ] {
        assert!(
            lower.contains(expected),
            "migration 150 is missing {expected}"
        );
    }
    for forbidden in [
        "alter table",
        "create table",
        "drop constraint",
        "insert into",
        "update asset_releases",
        "update build_runs",
        "delete from",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 150 changed authority instead of documenting the boundary: {forbidden}"
        );
    }
}
