const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/182_agent_release_manifests.sql"
));

#[test]
fn migration_182_retains_one_complete_bounded_agent_manifest() {
    for column in [
        "agent_manifest_identity",
        "agent_manifest_acl",
        "agent_manifest_archive_digest",
        "agent_manifest_archive_size_bytes",
        "agent_manifest_source_content_digest",
    ] {
        assert!(MIGRATION.contains(column));
    }
    assert!(MIGRATION.contains("asset_releases_agent_manifest_shape_check"));
    assert!(MIGRATION.contains("octet_length(agent_manifest_acl) between 1 and 65536"));
    assert!(MIGRATION.contains("state in ('published', 'yanked')"));
    assert!(MIGRATION.contains("artifact_kind = 'oci_service'"));
    assert!(MIGRATION.contains("agent_release_contract jsonb"));
    assert!(MIGRATION.contains("workload_revisions_agent_release_contract_shape_check"));
    assert!(MIGRATION.contains("pg_column_size(agent_release_contract) between 1 and 262144"));
}
