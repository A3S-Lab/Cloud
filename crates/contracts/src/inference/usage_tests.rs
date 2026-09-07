use super::*;
use base64::Engine;
use serde_json::json;
use sha2::{Digest, Sha256};

fn cursor(epoch: Uuid, sequence: u64) -> InferenceUsageCursorV1 {
    InferenceUsageCursorV1 {
        boot_epoch: epoch,
        sequence,
    }
}

fn record(epoch: Uuid, sequence: u64, payload: &[u8]) -> InferenceUsageRecordV1 {
    InferenceUsageRecordV1 {
        cursor: cursor(epoch, sequence),
        event_id: Uuid::new_v4(),
        payload_base64: base64::engine::general_purpose::STANDARD.encode(payload),
        payload_sha256: format!("{:x}", Sha256::digest(payload)),
    }
}

fn batch() -> InferenceUsageBatchV1 {
    let epoch = Uuid::from_u128(1);
    InferenceUsageBatchV1 {
        schema: INFERENCE_USAGE_BATCH_SCHEMA_V1.into(),
        gateway_id: Uuid::from_u128(2),
        batch_id: Uuid::from_u128(3),
        after: None,
        records: vec![
            record(epoch, 1, br#"{"kind":"request_started"}"#),
            record(epoch, 2, br#"{"kind":"request_terminal"}"#),
        ],
    }
}

#[test]
fn usage_batch_round_trips_the_gateway_wire_shape() {
    let batch = batch();
    let encoded = batch.encode().expect("valid usage batch");
    let decoded = InferenceUsageBatchV1::decode(&encoded).expect("decodable usage batch");
    assert_eq!(decoded, batch);
    let json = serde_json::from_slice::<serde_json::Value>(&encoded).expect("JSON");
    assert_eq!(
        json["records"][0]["payload_base64"],
        "eyJraW5kIjoicmVxdWVzdF9zdGFydGVkIn0="
    );
    assert!(json.get("prompt").is_none());
}

#[test]
fn usage_batch_rejects_tampering_unknown_fields_and_noncontiguous_cursors() {
    let valid = batch();
    let mut tampered = valid.clone();
    tampered.records[0].payload_sha256 = "0".repeat(64);
    assert!(tampered.validate().is_err());

    let mut unknown = serde_json::to_value(valid.clone()).expect("JSON");
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<InferenceUsageBatchV1>(unknown).is_err());

    let mut gap = valid;
    gap.records[1].cursor.sequence = 3;
    assert!(gap.validate().is_err());
}

#[test]
fn usage_batch_requires_after_to_be_immediately_followed() {
    let mut batch = batch();
    let after = batch.records[0].cursor;
    batch.after = Some(after);
    batch.records = vec![record(after.boot_epoch, after.sequence + 2, b"skipped")];
    assert!(batch.validate().is_err());
}

#[test]
fn usage_batch_rejects_invalid_payload_bounds_and_digest() {
    let mut invalid = batch();
    invalid.records[0].payload_base64 = "%%%".into();
    assert!(invalid.validate().is_err());

    let mut too_large = batch();
    too_large.records[0] = record(
        Uuid::from_u128(1),
        1,
        &vec![b'x'; INFERENCE_USAGE_MAX_EVENT_BYTES + 1],
    );
    assert!(too_large.validate().is_err());
}

#[test]
fn receipt_round_trips_and_rejects_foreign_or_gap_acknowledgements() {
    let batch = batch();
    let receipt = InferenceUsageReceiptV1 {
        schema: INFERENCE_USAGE_RECEIPT_SCHEMA_V1.into(),
        gateway_id: batch.gateway_id,
        batch_id: batch.batch_id,
        acknowledged_through: Some(batch.records[1].cursor),
        gaps: Vec::new(),
    };
    let encoded = receipt.encode_for(&batch).expect("valid receipt");
    assert_eq!(
        InferenceUsageReceiptV1::decode_for(&encoded, &batch).unwrap(),
        receipt
    );

    let mut foreign = receipt.clone();
    foreign.gateway_id = Uuid::new_v4();
    assert!(foreign.validate_for(&batch).is_err());

    let mut gap = receipt;
    gap.gaps.push(batch.records[1].cursor);
    assert!(gap.validate_for(&batch).is_err());
}

#[test]
fn receipt_accepts_bounded_gap_information_without_acknowledging_it() {
    let batch = batch();
    let receipt = InferenceUsageReceiptV1 {
        schema: INFERENCE_USAGE_RECEIPT_SCHEMA_V1.into(),
        gateway_id: batch.gateway_id,
        batch_id: batch.batch_id,
        acknowledged_through: None,
        gaps: vec![InferenceUsageCursorV1 {
            boot_epoch: Uuid::from_u128(9),
            sequence: 1,
        }],
    };
    receipt.validate_for(&batch).expect("valid gap receipt");
}

#[test]
fn usage_contract_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<InferenceUsageBatchV1>();
    assert_send_sync::<InferenceUsageReceiptV1>();
}
