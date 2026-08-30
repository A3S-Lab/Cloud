const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/181_workload_runtime_evidence_history.sql"
));

#[test]
fn migration_181_adds_one_immutable_non_authorizing_runtime_history() {
    let lower = MIGRATION.to_ascii_lowercase();
    for required in [
        "create table workload_runtime_evidence_history",
        "cloud.identity.workload-runtime-evidence-record.v1",
        "cloud.identity.workload-runtime-evidence-binding.v1",
        "workload runtime evidence history is immutable",
        "for key share of installation",
        "workload_identity_policy_heads",
        "trust_domain_heads",
        "policy_revision.digest = new.policy_digest",
        "node_attestation_binding_digest is null",
        "runtime_state = 'running'",
        "interval '120 seconds'",
        "never authorizes workload credential issuance",
    ] {
        assert!(
            lower.contains(required),
            "migration 181 is missing {required}"
        );
    }
    assert_eq!(
        lower
            .matches("create table workload_runtime_evidence_history")
            .count(),
        1
    );
    for forbidden in [
        "create table workload_runtime_evidence_head",
        "create table resource_claim",
        "create table node",
        "create table runtime_unit",
        "references resource_claims",
        "references nodes",
        "references runtime_units",
        "on delete cascade",
        "on delete set null",
        "redis",
        "a3s_lane",
        "create queue",
        "provider_config",
        "json_config",
        "yaml_config",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 181 duplicated or weakened an authority through {forbidden}"
        );
    }
}
