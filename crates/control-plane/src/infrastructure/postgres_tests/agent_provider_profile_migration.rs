const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/160_agent_provider_profiles.sql"
));

#[test]
fn migration_160_rebinds_code_runs_to_one_immutable_provider_contract() {
    let canonical = MIGRATION
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    for expected in [
        "rename column code_node_id to provider_node_id",
        "rename column code_run_id to provider_run_id",
        "add column provider_kind text",
        "add column provider_profile_acl text",
        "add column provider_profile_digest text",
        "add column provider_capability_digest text",
        "provider_protocol = 'a3s.cloud.agent-provider.v1'",
        "provider_profile_digest = 'sha256:7587d2e401ffe738c0416765ce3c5d1683ae853c533e10291b1b82029eabb926'",
        "provider_capability_digest = 'sha256:6a40014e842fc193722ac2cf2604532b3d88ec75132781f7c02edab55b8e976c'",
        "agent_executions_provider_binding_complete",
        "agent_executions_provider_binding_values",
        "agent_executions_provider_run_identity_unique",
        "drop constraint node_commands_command_kind_check",
        "'agent_provider_command'",
        "'code_agent_command'",
        "canonical immutable acl",
        "sole agent semantic event stream",
    ] {
        assert!(canonical.contains(expected), "missing {expected}");
    }
    for forbidden in [
        "create table",
        "create queue",
        "provider_secret",
        "provider_token",
        "provider_endpoint",
        "jsonb",
    ] {
        assert!(
            !canonical.contains(forbidden),
            "migration 160 added duplicate or mutable provider authority through {forbidden}"
        );
    }
}
