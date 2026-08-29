const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/178_tenant_support_grant_approvals.sql"
));

#[test]
fn migration_178_separates_acl_intent_from_actual_approval_and_grant_facts() {
    let lower = MIGRATION.to_ascii_lowercase();
    for required in [
        "create table tenant_support_grant_intents",
        "create table tenant_support_grant_required_approvers",
        "create table tenant_support_grant_approvals",
        "create table tenant_support_grants",
        "validate_cloud_fact_scope_lineage_at_insert()",
        "approver ids are requirements, not approval facts",
        "authentication_digest",
        "policy_revision_id",
        "binding_version",
        "evidence_digest",
        "deferrable initially deferred",
        "tenant support subject and requester cannot approve the grant",
        "tenant support approval lacks current human policy and binding evidence",
        "tenant support grant lacks complete live approval evidence",
        "select max(approval.approved_at)",
        "tenant support grant revocation must be one terminal generation",
    ] {
        assert!(
            lower.contains(required),
            "migration 178 is missing {required}"
        );
    }
    assert_eq!(
        lower
            .matches("execute function validate_cloud_fact_scope_lineage_at_insert()")
            .count(),
        1,
        "support intent must reuse the existing tenant lineage validator"
    );
    for forbidden in [
        "create table tenant_support_audit",
        "create table tenant_support_outbox",
        "create table tenant_support_idempotency",
        "create table tenant_support_locks",
        "redis",
        "a3s_lane",
        "on delete cascade",
        "on delete set null",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 178 duplicated or weakened an existing authority through {forbidden}"
        );
    }
}
