const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/192_inference_usage_ledger.sql"
));

#[test]
fn migration_192_retains_watermark_and_event_digest_ledger_only() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table inference_usage_watermarks",
        "create table inference_usage_events",
        "payload_sha256",
        "primary key (organization_id, gateway_id, event_id)",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 192 is missing {expected}"
        );
    }
    for forbidden in [
        "payload_base64",
        "bearer",
        "api_key",
        "token_count",
        "billing_amount",
        "raw_payload",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 192 introduced out-of-scope storage: {forbidden}"
        );
    }
}
