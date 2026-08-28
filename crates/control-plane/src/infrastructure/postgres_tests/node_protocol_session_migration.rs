const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/172_node_protocol_session_heads.sql"
));

#[test]
fn migration_adds_one_fleet_owned_current_session_head() {
    assert!(MIGRATION.contains("create table node_protocol_session_heads"));
    assert!(MIGRATION.contains("node_id uuid primary key"));
    assert!(MIGRATION.contains("references nodes (organization_id, id)"));
    assert!(MIGRATION.contains("generation between 1 and 9007199254740991"));
    assert!(MIGRATION.contains("expires_at <= selected_at + interval '24 hours'"));
    assert!(MIGRATION.contains("a3s.cloud.node-session-hello.v1"));
    assert!(MIGRATION.contains("a3s.cloud.node-session-selection.v1"));
    assert!(!MIGRATION.contains("prompt"));
    assert!(!MIGRATION.contains("kv_cache"));
}
