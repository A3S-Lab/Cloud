const MIGRATION_192: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/192_inference_usage_ledger.sql"
));

const MIGRATION_193: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/193_inference_usage_lifecycle_payload.sql"
));

const MIGRATION_194: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/194_inference_usage_request_facts_and_rollups.sql"
));

#[test]
fn migration_192_retains_watermark_and_event_digest_ledger_only() {
    let lower = MIGRATION_192.to_ascii_lowercase();
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

#[test]
fn migration_193_adds_prompt_free_lifecycle_payload_bytes() {
    let lower = MIGRATION_193.to_ascii_lowercase();
    assert!(lower.contains("add column payload bytea"));
    assert!(lower.contains("inference_usage_events_payload_not_empty"));
    for forbidden in ["messages", "api_key", "billing_amount", "payload_base64"] {
        assert!(
            !lower.contains(forbidden),
            "migration 193 introduced out-of-scope storage: {forbidden}"
        );
    }
}

#[test]
fn migration_194_adds_request_facts_and_rebuildable_daily_rollups() {
    let lower = MIGRATION_194.to_ascii_lowercase();
    for expected in [
        "create table inference_usage_request_facts",
        "create table inference_usage_daily_rollups",
        "primary key (organization_id, request_id)",
        "primary key (\n        organization_id,\n        day,\n        environment_id,\n        model_id,\n        endpoint\n    )",
        "attempt_count",
        "unknown_measurement_count",
        "upstream_usage_count",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 194 is missing {expected}"
        );
    }
    for forbidden in [
        "prompt",
        "messages",
        "api_key",
        "billing_amount",
        "invoice",
        "balance",
        "price",
        "payload_base64",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 194 introduced out-of-scope storage: {forbidden}"
        );
    }
}
