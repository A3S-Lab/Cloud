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

const MIGRATION_195: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/195_inference_usage_retention_authority.sql"
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

#[test]
fn migration_195_establishes_monotonic_inference_usage_retention_authority() {
    let canonical = MIGRATION_195
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for expected in [
        "create table inference_usage_retention_states",
        "organization_id uuid primary key references organizations(id) on delete cascade",
        "records_available_from timestamptz",
        "records_deleted_before timestamptz",
        "insert into inference_usage_retention_states (organization_id) select id from organizations",
        "organizations_create_inference_usage_retention_state",
        "inference_usage_retention_states_monotonic",
        "inference_usage_events_enforce_retention_boundary",
        "inference_usage_request_facts_enforce_retention_boundary",
        "new.version <> old.version + 1",
        "watermarks are never deleted",
    ] {
        assert!(
            canonical.contains(expected),
            "migration 195 is missing {expected}"
        );
    }
    for forbidden in ["billing_amount", "prompt", "api_key", "invoice"] {
        assert!(
            !canonical.contains(forbidden),
            "migration 195 introduced out-of-scope storage: {forbidden}"
        );
    }
}
