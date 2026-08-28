const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/174_installation_scoped_facts.sql"
));

#[test]
fn migration_174_adds_one_installation_identity_and_evolves_the_shared_fact_rail() {
    let lower = MIGRATION.to_ascii_lowercase();
    for required in [
        "create table cloud_installations",
        "singleton_key boolean primary key",
        "cloud_installations_immutable",
        "current_cloud_installation_id()",
        "alter table organizations",
        "organizations_installation_identity_unique",
        "alter table outbox_events",
        "alter table audit_records",
        "scope_kind = 'installation'",
        "scope_kind = 'organization'",
        "scope_kind = 'project'",
        "scope_kind = 'environment'",
        "outbox_events_organization_scope_fk",
        "audit_records_organization_scope_fk",
        "create function cloud_scope_document",
        "reject_cloud_fact_scope_mutation",
        "installation audit is retained indefinitely",
        "alter column scope_kind set default 'organization'",
    ] {
        assert!(
            lower.contains(required),
            "migration 174 is missing {required}"
        );
    }

    assert_eq!(lower.matches("create table cloud_installations").count(), 1);
    for duplicate in [
        "create table installation_outbox",
        "create table platform_outbox",
        "create table installation_audit",
        "create table platform_audit",
        "create table tenant_outbox",
        "create table tenant_audit",
    ] {
        assert!(
            !lower.contains(duplicate),
            "migration 174 introduced a second Audit/Outbox mechanism through {duplicate}"
        );
    }
}
