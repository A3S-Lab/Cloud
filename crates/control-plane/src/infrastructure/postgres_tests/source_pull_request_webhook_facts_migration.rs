const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/156_source_pull_request_webhook_facts.sql"
));

#[test]
fn migration_156_extends_the_single_sources_inbox_without_new_delivery_mechanisms() {
    let migration = MIGRATION.to_ascii_lowercase();

    assert_eq!(
        migration
            .matches("alter table source_webhook_inbox")
            .count(),
        2
    );
    for required in [
        "event_kind in ('push', 'pull_request')",
        "source_webhook_inbox_typed_payload_check",
        "pull_request_change_kind",
        "provider_updated_at >= provider_created_at",
    ] {
        assert!(
            migration.contains(required),
            "missing invariant: {required}"
        );
    }
    for duplicate_mechanism in [
        "create table",
        "create function",
        "create trigger",
        "create index",
        "alter table outbox_events",
        "alter table external_source_revisions",
        "alter table operation_requests",
    ] {
        assert!(
            !migration.contains(duplicate_mechanism),
            "migration 156 introduced another delivery or lifecycle mechanism through {duplicate_mechanism}"
        );
    }
}
