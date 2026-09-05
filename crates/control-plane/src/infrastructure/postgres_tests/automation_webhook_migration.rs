const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/183_automation_webhook_admission.sql"
));

#[test]
fn migration_183_persists_exact_endpoint_and_immutable_delivery_receipts() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table automation_webhook_endpoints",
        "create table automation_webhook_deliveries",
        "create table automation_webhook_delivery_receipts",
        "revision_acl",
        "endpoint_json",
        "endpoint_key ~ '^[a-z0-9_-]+$'",
        "unique (organization_id, project_id, environment_id, endpoint_key)",
        "unique (endpoint_id)",
        "validate_automation_webhook_endpoint_transition",
        "automation_webhook_deliveries_immutable",
        "automation_webhook_delivery_receipts_immutable",
        "foreign key (organization_id, endpoint_id, delivery_id)",
        "stateChangedAt",
        "secret plaintext",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 183 is missing {expected}"
        );
    }
    for forbidden in [
        "secret_material",
        "secret_value",
        "create table automation_webhook_scheduler",
        "retry_count",
        "next_attempt",
        "http_request",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 183 introduced an out-of-scope authority: {forbidden}"
        );
    }
}
