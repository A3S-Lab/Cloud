const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/154_connector_revision_revocations.sql"
));

#[test]
fn migration_154_serializes_exact_revision_revocation_with_dispatch_admission() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table connector_revision_revocations",
        "primary key (organization_id, profile_id, revision_id)",
        "references connector_revisions",
        "for update",
        "connector revision revocation does not match its exact revision",
        "before update or delete on connector_revision_revocations",
        "connector revision revocations are immutable",
        "serialized with provider dispatch admission",
    ] {
        assert!(
            lower.contains(expected),
            "migration 154 is missing {expected}"
        );
    }
    for forbidden in [
        "update connector_revisions",
        "delete from connector_revisions",
        "retry_count",
        "provider_state",
        "secret_material",
        "flow_history",
    ] {
        assert!(
            !lower.contains(forbidden),
            "revision revocation acquired another authority through {forbidden}"
        );
    }
}
