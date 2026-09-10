//! Pure Inference usage-ledger apply rules shared by in-memory and Postgres.
//!
//! Cloud owns the durable ledger. This module encodes only the contiguous ACK /
//! wrong-`after` / event-id digest conflict invariants from
//! `a3s.gateway.usage-batch.v1` so persistence adapters cannot drift.

use a3s_cloud_contracts::{
    InferenceUsageBatchV1, InferenceUsageCursorV1, InferenceUsageLifecycleEventV1,
    InferenceUsageReceiptV1, INFERENCE_USAGE_RECEIPT_SCHEMA_V1,
};
use std::collections::HashMap;
use uuid::Uuid;

/// Mutable ledger tip for one `(organization, gateway)` stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InferenceUsageLedgerState {
    pub watermark: Option<InferenceUsageCursorV1>,
    /// `event_id` → lowercase hex `payload_sha256`.
    pub events: HashMap<Uuid, String>,
}

/// Outcome of applying one validated batch to an in-memory ledger view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceUsageLedgerApply {
    pub receipt: InferenceUsageReceiptV1,
    pub next: InferenceUsageLedgerState,
    /// Event ids newly recorded by this apply (exact redeliveries excluded).
    pub inserted_event_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceUsageLedgerError {
    Contract(String),
    Conflict(String),
}

impl InferenceUsageLedgerError {
    pub fn into_message(self) -> String {
        match self {
            Self::Contract(message) | Self::Conflict(message) => message,
        }
    }
}

/// Choose a wrong-`after` acknowledgement that satisfies
/// [`InferenceUsageReceiptV1::validate_for`].
fn contract_valid_hold_ack(
    watermark: Option<InferenceUsageCursorV1>,
    batch: &InferenceUsageBatchV1,
) -> Option<InferenceUsageCursorV1> {
    if watermark == batch.after
        || watermark.is_some_and(|tip| batch.records.iter().any(|record| record.cursor == tip))
    {
        watermark
    } else {
        batch.after
    }
}

/// Apply `batch` to `state` without side effects.
///
/// Wrong-`after` holds the watermark and never invents contiguity. Gaps never
/// include the acknowledgement cursor. Matching event-id redelivery is
/// idempotent; digest mismatch is a conflict.
pub fn apply_inference_usage_batch(
    state: &InferenceUsageLedgerState,
    batch: &InferenceUsageBatchV1,
) -> Result<InferenceUsageLedgerApply, InferenceUsageLedgerError> {
    batch
        .validate()
        .map_err(InferenceUsageLedgerError::Contract)?;
    for record in &batch.records {
        let payload = record
            .payload()
            .map_err(InferenceUsageLedgerError::Contract)?;
        InferenceUsageLifecycleEventV1::decode(&payload)
            .map_err(InferenceUsageLedgerError::Contract)?;
    }

    if batch.after != state.watermark {
        // Receipts may only acknowledge `after` or a cursor carried in this
        // batch. When the durable tip is ahead of the submitted window, hold
        // without advertising an out-of-batch watermark (Gateway rejects that).
        let acknowledged_through = contract_valid_hold_ack(state.watermark, batch);
        let receipt = InferenceUsageReceiptV1 {
            schema: INFERENCE_USAGE_RECEIPT_SCHEMA_V1.into(),
            gateway_id: batch.gateway_id,
            batch_id: batch.batch_id,
            acknowledged_through,
            gaps: Vec::new(),
        };
        receipt
            .validate_for(batch)
            .map_err(InferenceUsageLedgerError::Contract)?;
        return Ok(InferenceUsageLedgerApply {
            receipt,
            next: state.clone(),
            inserted_event_ids: Vec::new(),
        });
    }

    let mut next = state.clone();
    let mut acknowledged_through = next.watermark;
    let mut gaps = Vec::new();
    let mut inserted_event_ids = Vec::new();

    for record in &batch.records {
        let expected = match acknowledged_through {
            None => InferenceUsageCursorV1 {
                boot_epoch: record.cursor.boot_epoch,
                sequence: 1,
            },
            Some(cursor) if cursor.boot_epoch == record.cursor.boot_epoch => {
                InferenceUsageCursorV1 {
                    boot_epoch: cursor.boot_epoch,
                    sequence: cursor.sequence.saturating_add(1),
                }
            }
            Some(_) => record.cursor,
        };

        let new_epoch_start = acknowledged_through.is_some_and(|cursor| {
            cursor.boot_epoch != record.cursor.boot_epoch && record.cursor.sequence == 1
        });
        if record.cursor != expected && !new_epoch_start {
            gaps.push(expected);
            break;
        }

        match next.events.get(&record.event_id) {
            Some(digest) if digest != &record.payload_sha256 => {
                return Err(InferenceUsageLedgerError::Conflict(format!(
                    "usage event {} payload digest conflicts with the ledger",
                    record.event_id
                )));
            }
            Some(_) => {}
            None => {
                next.events
                    .insert(record.event_id, record.payload_sha256.clone());
                inserted_event_ids.push(record.event_id);
            }
        }
        acknowledged_through = Some(record.cursor);
    }

    next.watermark = acknowledged_through;
    let receipt = InferenceUsageReceiptV1 {
        schema: INFERENCE_USAGE_RECEIPT_SCHEMA_V1.into(),
        gateway_id: batch.gateway_id,
        batch_id: batch.batch_id,
        acknowledged_through,
        gaps,
    };
    receipt
        .validate_for(batch)
        .map_err(InferenceUsageLedgerError::Contract)?;
    Ok(InferenceUsageLedgerApply {
        receipt,
        next,
        inserted_event_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{
        InferenceUsageEndpointV1, InferenceUsageLifecycleEventV1, InferenceUsageLifecycleKindV1,
        InferenceUsageRecordV1, InferenceUsageRequestEvidenceV1,
    };
    use base64::Engine;
    use chrono::{DateTime, Utc};
    use sha2::{Digest, Sha256};

    fn lifecycle_payload() -> Vec<u8> {
        let event = InferenceUsageLifecycleEventV1 {
            schema: InferenceUsageLifecycleEventV1::SCHEMA.into(),
            kind: InferenceUsageLifecycleKindV1::RequestStarted,
            occurred_at: DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            request: InferenceUsageRequestEvidenceV1 {
                request_id: Uuid::from_u128(100),
                correlation_id: "corr".into(),
                environment_id: Uuid::from_u128(101),
                credential_id: Uuid::from_u128(102),
                credential_generation: 1,
                route_id: Uuid::from_u128(103),
                route_policy_revision: 1,
                endpoint: InferenceUsageEndpointV1::ChatCompletions,
                model_alias: "alias".into(),
                model_id: Uuid::from_u128(104),
            },
            attempt: None,
            outcome: None,
            http_status: None,
            duration_ms: None,
            measurement_completeness: None,
            total_tokens: None,
        };
        serde_json::to_vec(&event).unwrap()
    }

    fn record(cursor: InferenceUsageCursorV1, event_id: Uuid) -> InferenceUsageRecordV1 {
        let payload = lifecycle_payload();
        InferenceUsageRecordV1 {
            cursor,
            event_id,
            payload_base64: base64::engine::general_purpose::STANDARD.encode(&payload),
            payload_sha256: format!("{:x}", Sha256::digest(&payload)),
        }
    }

    #[test]
    fn wrong_after_holds_watermark_without_gaps() {
        let epoch = Uuid::from_u128(1);
        let gateway_id = Uuid::from_u128(2);
        let tip = InferenceUsageCursorV1 {
            boot_epoch: epoch,
            sequence: 1,
        };
        let state = InferenceUsageLedgerState {
            watermark: Some(tip),
            events: HashMap::new(),
        };
        let batch = InferenceUsageBatchV1 {
            schema: InferenceUsageBatchV1::SCHEMA.into(),
            gateway_id,
            batch_id: Uuid::from_u128(3),
            after: None,
            records: vec![record(tip, Uuid::from_u128(10))],
        };
        let applied = apply_inference_usage_batch(&state, &batch).unwrap();
        assert_eq!(applied.receipt.acknowledged_through, Some(tip));
        assert!(applied.receipt.gaps.is_empty());
        assert!(applied.inserted_event_ids.is_empty());
        assert_eq!(applied.next, state);
    }

    #[test]
    fn wrong_after_does_not_advertise_tip_outside_batch() {
        let epoch = Uuid::from_u128(1);
        let gateway_id = Uuid::from_u128(2);
        let tip = InferenceUsageCursorV1 {
            boot_epoch: epoch,
            sequence: 1,
        };
        let ahead = InferenceUsageCursorV1 {
            boot_epoch: epoch,
            sequence: 2,
        };
        let state = InferenceUsageLedgerState {
            watermark: Some(ahead),
            events: HashMap::new(),
        };
        let batch = InferenceUsageBatchV1 {
            schema: InferenceUsageBatchV1::SCHEMA.into(),
            gateway_id,
            batch_id: Uuid::from_u128(3),
            after: None,
            records: vec![record(tip, Uuid::from_u128(10))],
        };
        let applied = apply_inference_usage_batch(&state, &batch).unwrap();
        assert_eq!(applied.receipt.acknowledged_through, None);
        assert!(applied.receipt.gaps.is_empty());
        assert_eq!(applied.next.watermark, Some(ahead));
    }

    #[test]
    fn stale_after_with_tip_in_batch_returns_durable_watermark() {
        let epoch = Uuid::from_u128(1);
        let gateway_id = Uuid::from_u128(2);
        let tip = InferenceUsageCursorV1 {
            boot_epoch: epoch,
            sequence: 1,
        };
        let ahead = InferenceUsageCursorV1 {
            boot_epoch: epoch,
            sequence: 2,
        };
        let state = InferenceUsageLedgerState {
            watermark: Some(ahead),
            events: HashMap::new(),
        };
        let batch = InferenceUsageBatchV1 {
            schema: InferenceUsageBatchV1::SCHEMA.into(),
            gateway_id,
            batch_id: Uuid::from_u128(4),
            after: Some(tip),
            records: vec![record(ahead, Uuid::from_u128(11))],
        };
        let applied = apply_inference_usage_batch(&state, &batch).unwrap();
        assert_eq!(applied.receipt.acknowledged_through, Some(ahead));
        assert!(applied.receipt.gaps.is_empty());
        assert_eq!(applied.next, state);
    }

    #[test]
    fn contiguous_batch_advances_watermark() {
        let epoch = Uuid::from_u128(1);
        let gateway_id = Uuid::from_u128(2);
        let state = InferenceUsageLedgerState::default();
        let batch = InferenceUsageBatchV1 {
            schema: InferenceUsageBatchV1::SCHEMA.into(),
            gateway_id,
            batch_id: Uuid::from_u128(3),
            after: None,
            records: vec![record(
                InferenceUsageCursorV1 {
                    boot_epoch: epoch,
                    sequence: 1,
                },
                Uuid::from_u128(10),
            )],
        };
        let applied = apply_inference_usage_batch(&state, &batch).unwrap();
        assert_eq!(
            applied.receipt.acknowledged_through,
            Some(InferenceUsageCursorV1 {
                boot_epoch: epoch,
                sequence: 1
            })
        );
        assert_eq!(applied.inserted_event_ids, vec![Uuid::from_u128(10)]);
    }

    #[test]
    fn empty_stream_hole_advertises_gap_and_replay_advances_without_double_insert() {
        // First principles: an empty ledger never invents missing spool prefixes.
        // A batch that starts above sequence 1 must advertise the missing cursor,
        // hold the watermark, and insert nothing. Filling the prefix then the
        // previously-skipped tip advances ACK without double-counting digests.
        let epoch = Uuid::from_u128(1);
        let gateway_id = Uuid::from_u128(2);
        let state = InferenceUsageLedgerState::default();
        let skipped = InferenceUsageCursorV1 {
            boot_epoch: epoch,
            sequence: 3,
        };
        let hole = InferenceUsageBatchV1 {
            schema: InferenceUsageBatchV1::SCHEMA.into(),
            gateway_id,
            batch_id: Uuid::from_u128(30),
            after: None,
            records: vec![record(skipped, Uuid::from_u128(30))],
        };
        let advertised = apply_inference_usage_batch(&state, &hole).unwrap();
        assert_eq!(advertised.receipt.acknowledged_through, None);
        assert_eq!(
            advertised.receipt.gaps,
            vec![InferenceUsageCursorV1 {
                boot_epoch: epoch,
                sequence: 1
            }]
        );
        assert!(advertised.inserted_event_ids.is_empty());
        assert_eq!(advertised.next, state);
        advertised
            .receipt
            .validate_for(&hole)
            .expect("gap receipt must remain contract-valid");

        let fill = InferenceUsageBatchV1 {
            schema: InferenceUsageBatchV1::SCHEMA.into(),
            gateway_id,
            batch_id: Uuid::from_u128(31),
            after: None,
            records: vec![
                record(
                    InferenceUsageCursorV1 {
                        boot_epoch: epoch,
                        sequence: 1,
                    },
                    Uuid::from_u128(31),
                ),
                record(
                    InferenceUsageCursorV1 {
                        boot_epoch: epoch,
                        sequence: 2,
                    },
                    Uuid::from_u128(32),
                ),
                record(skipped, Uuid::from_u128(30)),
            ],
        };
        let advanced = apply_inference_usage_batch(&advertised.next, &fill).unwrap();
        assert_eq!(advanced.receipt.acknowledged_through, Some(skipped));
        assert!(advanced.receipt.gaps.is_empty());
        assert_eq!(
            advanced.inserted_event_ids,
            vec![
                Uuid::from_u128(31),
                Uuid::from_u128(32),
                Uuid::from_u128(30)
            ]
        );
        assert_eq!(advanced.next.events.len(), 3);

        // Exact redelivery of the filled window must not invent a fourth event.
        let redelivery = apply_inference_usage_batch(&advanced.next, &fill).unwrap();
        assert_eq!(redelivery.receipt.acknowledged_through, Some(skipped));
        assert!(redelivery.inserted_event_ids.is_empty());
        assert_eq!(redelivery.next.events.len(), 3);
    }

    #[test]
    fn multi_record_continuation_after_matching_tip_advances_watermark() {
        let epoch = Uuid::from_u128(1);
        let gateway_id = Uuid::from_u128(2);
        let tip = InferenceUsageCursorV1 {
            boot_epoch: epoch,
            sequence: 2,
        };
        let mut state = InferenceUsageLedgerState {
            watermark: Some(tip),
            events: HashMap::new(),
        };
        state
            .events
            .insert(Uuid::from_u128(20), "deadbeef".repeat(8));
        let batch = InferenceUsageBatchV1 {
            schema: InferenceUsageBatchV1::SCHEMA.into(),
            gateway_id,
            batch_id: Uuid::from_u128(40),
            after: Some(tip),
            records: vec![
                record(
                    InferenceUsageCursorV1 {
                        boot_epoch: epoch,
                        sequence: 3,
                    },
                    Uuid::from_u128(41),
                ),
                record(
                    InferenceUsageCursorV1 {
                        boot_epoch: epoch,
                        sequence: 4,
                    },
                    Uuid::from_u128(42),
                ),
            ],
        };
        let applied = apply_inference_usage_batch(&state, &batch).unwrap();
        assert_eq!(
            applied.receipt.acknowledged_through,
            Some(InferenceUsageCursorV1 {
                boot_epoch: epoch,
                sequence: 4
            })
        );
        assert!(applied.receipt.gaps.is_empty());
        assert_eq!(
            applied.inserted_event_ids,
            vec![Uuid::from_u128(41), Uuid::from_u128(42)]
        );
    }

    #[test]
    fn digest_conflict_is_rejected() {
        let epoch = Uuid::from_u128(1);
        let event_id = Uuid::from_u128(10);
        let cursor = InferenceUsageCursorV1 {
            boot_epoch: epoch,
            sequence: 1,
        };
        let first = record(cursor, event_id);
        let mut state = InferenceUsageLedgerState::default();
        state.events.insert(
            event_id,
            format!("{:x}", Sha256::digest(b"other-digest-payload")),
        );
        let batch = InferenceUsageBatchV1 {
            schema: InferenceUsageBatchV1::SCHEMA.into(),
            gateway_id: Uuid::from_u128(2),
            batch_id: Uuid::from_u128(3),
            after: None,
            records: vec![first],
        };
        let err = apply_inference_usage_batch(&state, &batch).unwrap_err();
        assert!(matches!(err, InferenceUsageLedgerError::Conflict(_)));
    }

    #[test]
    fn invalid_lifecycle_payload_is_rejected() {
        let epoch = Uuid::from_u128(1);
        let cursor = InferenceUsageCursorV1 {
            boot_epoch: epoch,
            sequence: 1,
        };
        let payload = br#"{"kind":"not-a-lifecycle"}"#;
        let batch = InferenceUsageBatchV1 {
            schema: InferenceUsageBatchV1::SCHEMA.into(),
            gateway_id: Uuid::from_u128(2),
            batch_id: Uuid::from_u128(3),
            after: None,
            records: vec![InferenceUsageRecordV1 {
                cursor,
                event_id: Uuid::from_u128(10),
                payload_base64: base64::engine::general_purpose::STANDARD.encode(payload),
                payload_sha256: format!("{:x}", Sha256::digest(payload)),
            }],
        };
        let err =
            apply_inference_usage_batch(&InferenceUsageLedgerState::default(), &batch).unwrap_err();
        assert!(matches!(err, InferenceUsageLedgerError::Contract(_)));
    }
}
