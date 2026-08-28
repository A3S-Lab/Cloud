const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/174_installation_scoped_facts.sql"
));
const ROLLING_COMPATIBILITY_MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/175_legacy_scoped_fact_writer_compatibility.sql"
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

#[test]
fn migration_175_reuses_one_fail_closed_scope_derivation_seam_for_legacy_tenant_writers() {
    let lower = ROLLING_COMPATIBILITY_MIGRATION.to_ascii_lowercase();
    for required in [
        "create function derive_legacy_tenant_fact_scope_kind()",
        "if new.scope_kind is not null",
        "if new.organization_id is null",
        "scope_kind must be explicit for installation facts",
        "when new.environment_id is not null then 'environment'",
        "when new.project_id is not null then 'project'",
        "else 'organization'",
        "outbox_events_derive_legacy_tenant_scope",
        "audit_records_derive_legacy_tenant_scope",
        "alter column scope_kind drop default",
        "one bounded rolling-upgrade seam",
    ] {
        assert!(
            lower.contains(required),
            "migration 175 is missing {required}"
        );
    }
    assert_eq!(
        lower
            .matches("create function derive_legacy_tenant_fact_scope_kind()")
            .count(),
        1,
        "rolling compatibility introduced more than one scope derivation mechanism"
    );
    assert_eq!(
        lower
            .matches("execute function derive_legacy_tenant_fact_scope_kind()")
            .count(),
        2,
        "Outbox and Audit must reuse the same legacy scope derivation function"
    );
    for forbidden in [
        "create table installation_outbox",
        "create table platform_outbox",
        "create table installation_audit",
        "create table platform_audit",
        "alter table outbox_events drop constraint outbox_events_scope_shape",
        "alter table audit_records drop constraint audit_records_scope_shape",
        "alter table audit_records drop constraint audit_records_scope_attribution_shape",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 175 bypassed the shared fact rail through {forbidden}"
        );
    }
}
