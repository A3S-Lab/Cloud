const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/177_platform_rbac_authority.sql"
));

#[test]
fn migration_177_establishes_one_strongly_consistent_identity_owned_platform_rbac_authority() {
    let lower = MIGRATION.to_ascii_lowercase();
    for required in [
        "create table platform_role_policy_revisions",
        "create table platform_role_policy_heads",
        "create table platform_role_bindings",
        "references cloud_installations",
        "references identity_principals",
        "platform_role_bindings_active_principal_unique",
        "for update of installation",
        "platform role policy head must advance to its exact successor",
        "the last active platform owner cannot be removed",
        "the last active platform owner principal cannot be disabled",
        "deferrable initially deferred",
        "platform rbac authority must retain an active platform owner",
        "platform role bindings require one current policy head",
        "accepted platform role policy revisions are immutable",
        "platform role binding history is not deletable",
    ] {
        assert!(
            lower.contains(required),
            "migration 177 is missing {required}"
        );
    }

    assert_eq!(
        lower
            .matches("create table platform_role_policy_heads")
            .count(),
        1,
        "there must be one current-head mechanism"
    );
    for forbidden in [
        "create table platform_audit",
        "create table platform_outbox",
        "create table platform_idempotency",
        "create table platform_distributed_locks",
        "create table platform_role_permissions",
        "redis",
        "a3s_lane",
        "on delete cascade",
        "on delete set null",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 177 duplicated or weakened an existing authority through {forbidden}"
        );
    }
}
