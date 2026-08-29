const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/179_workload_trust_authority.sql"
));

#[test]
fn migration_179_establishes_one_revisioned_workload_trust_authority() {
    let lower = MIGRATION.to_ascii_lowercase();
    for required in [
        "create table trust_domain_revisions",
        "create table trust_domain_heads",
        "create table workload_identity_policy_revisions",
        "create table workload_identity_policy_heads",
        "references cloud_installations",
        "references organizations",
        "references environments",
        "references workloads",
        "references workload_revisions",
        "references node_pools",
        "for update of installation",
        "accepted trust-domain revisions are immutable",
        "trust-domain predecessor is not the exact current head",
        "workload identity policy must bind the exact current trust-domain revision",
        "accepted workload identity policy revisions are immutable",
        "workload identity policy predecessor is not the exact current head",
        "unique (policy_id)",
        "unique (organization_id, workload_id)",
    ] {
        assert!(
            lower.contains(required),
            "migration 179 is missing {required}"
        );
    }
    assert_eq!(lower.matches("create table trust_domain_heads").count(), 1);
    assert_eq!(
        lower
            .matches("create table workload_identity_policy_heads")
            .count(),
        1
    );
    for forbidden in [
        "create table workload_trust_audit",
        "create table workload_trust_outbox",
        "create table workload_trust_idempotency",
        "create table workload_trust_locks",
        "create table workload_trust_permissions",
        "redis",
        "a3s_lane",
        "on delete cascade",
        "on delete set null",
        "\\x00",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 179 duplicated or weakened an authority through {forbidden}"
        );
    }
}
