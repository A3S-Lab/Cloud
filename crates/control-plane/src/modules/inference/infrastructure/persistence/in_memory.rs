use crate::modules::inference::domain::{
    AcceptInferenceUsageBatchWrite, IInferenceUsageRepository,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::{
    InferenceUsageBatchV1, InferenceUsageCursorV1, InferenceUsageReceiptV1,
    INFERENCE_USAGE_RECEIPT_SCHEMA_V1,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Default)]
struct GatewayLedgerState {
    watermark: Option<InferenceUsageCursorV1>,
    events: HashMap<Uuid, String>,
}

/// Process-local Inference usage ledger used until durable Postgres lands.
///
/// Semantics match the Gateway/Cloud usage-batch receipt contract: highest
/// contiguous acknowledgement, wrong-`after` gaps, and event-id digest conflicts.
#[derive(Debug, Default)]
pub struct InMemoryInferenceUsageRepository {
    gateways: Mutex<HashMap<(Uuid, Uuid), GatewayLedgerState>>,
}

impl InMemoryInferenceUsageRepository {
    pub fn new() -> Self {
        Self::default()
    }

    fn apply_batch(
        &self,
        organization_id: Uuid,
        batch: &InferenceUsageBatchV1,
    ) -> Result<InferenceUsageReceiptV1, RepositoryError> {
        batch
            .validate()
            .map_err(|error| RepositoryError::Storage(error))?;
        let mut gateways = self.gateways.lock().map_err(|_| {
            RepositoryError::Storage("inference usage ledger lock poisoned".into())
        })?;
        let state = gateways
            .entry((organization_id, batch.gateway_id))
            .or_default();

        if batch.after != state.watermark {
            // Caller is not continuing from the ledger tip. Hold the watermark
            // and do not invent contiguity. Gaps must not include the
            // acknowledgement cursor (contract invariant).
            let receipt = InferenceUsageReceiptV1 {
                schema: INFERENCE_USAGE_RECEIPT_SCHEMA_V1.into(),
                gateway_id: batch.gateway_id,
                batch_id: batch.batch_id,
                acknowledged_through: state.watermark,
                gaps: Vec::new(),
            };
            receipt
                .validate_for(batch)
                .map_err(|error| RepositoryError::Storage(error))?;
            return Ok(receipt);
        }

        let mut acknowledged_through = state.watermark;
        let mut gaps = Vec::new();
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

            match state.events.get(&record.event_id) {
                Some(digest) if digest != &record.payload_sha256 => {
                    return Err(RepositoryError::Conflict(format!(
                        "usage event {} payload digest conflicts with the ledger",
                        record.event_id
                    )));
                }
                Some(_) => {}
                None => {
                    state
                        .events
                        .insert(record.event_id, record.payload_sha256.clone());
                }
            }
            acknowledged_through = Some(record.cursor);
        }

        state.watermark = acknowledged_through;
        let receipt = InferenceUsageReceiptV1 {
            schema: INFERENCE_USAGE_RECEIPT_SCHEMA_V1.into(),
            gateway_id: batch.gateway_id,
            batch_id: batch.batch_id,
            acknowledged_through,
            gaps,
        };
        receipt
            .validate_for(batch)
            .map_err(|error| RepositoryError::Storage(error))?;
        Ok(receipt)
    }
}

#[async_trait]
impl IInferenceUsageRepository for InMemoryInferenceUsageRepository {
    async fn accept_usage_batch(
        &self,
        write: AcceptInferenceUsageBatchWrite,
    ) -> Result<InferenceUsageReceiptV1, RepositoryError> {
        write
            .validate()
            .map_err(|error| RepositoryError::Storage(error))?;
        self.apply_batch(write.organization_id.as_uuid(), &write.batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{NodeId, OrganizationId};
    use a3s_cloud_contracts::InferenceUsageRecordV1;
    use base64::Engine;
    use chrono::Utc;
    use sha2::{Digest, Sha256};

    fn record(cursor: InferenceUsageCursorV1, event_id: Uuid, payload: &[u8]) -> InferenceUsageRecordV1 {
        InferenceUsageRecordV1 {
            cursor,
            event_id,
            payload_base64: base64::engine::general_purpose::STANDARD.encode(payload),
            payload_sha256: format!("{:x}", Sha256::digest(payload)),
        }
    }

    #[tokio::test]
    async fn accepts_contiguous_batch_and_refuses_wrong_after() {
        let repo = InMemoryInferenceUsageRepository::new();
        let organization_id = OrganizationId::from_uuid(Uuid::from_u128(9));
        let node_id = NodeId::from_uuid(Uuid::from_u128(8));
        let gateway_id = Uuid::from_u128(2);
        let epoch = Uuid::from_u128(1);
        let first = InferenceUsageBatchV1 {
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
                b"a",
            )],
        };
        let receipt = repo
            .accept_usage_batch(
                AcceptInferenceUsageBatchWrite::new(
                    organization_id,
                    node_id,
                    first.clone(),
                    Utc::now(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            receipt.acknowledged_through,
            Some(InferenceUsageCursorV1 {
                boot_epoch: epoch,
                sequence: 1
            })
        );

        let replay = InferenceUsageBatchV1 {
            schema: InferenceUsageBatchV1::SCHEMA.into(),
            gateway_id,
            batch_id: Uuid::from_u128(4),
            after: None,
            records: vec![first.records[0].clone()],
        };
        let gap = repo
            .accept_usage_batch(
                AcceptInferenceUsageBatchWrite::new(organization_id, node_id, replay, Utc::now())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(gap.acknowledged_through, receipt.acknowledged_through);
        assert!(gap.gaps.is_empty());
    }

    #[tokio::test]
    async fn rejects_event_id_digest_conflicts() {
        let repo = InMemoryInferenceUsageRepository::new();
        let organization_id = OrganizationId::from_uuid(Uuid::new_v4());
        let node_id = NodeId::from_uuid(Uuid::new_v4());
        let gateway_id = Uuid::new_v4();
        let epoch = Uuid::new_v4();
        let event_id = Uuid::new_v4();
        let cursor = InferenceUsageCursorV1 {
            boot_epoch: epoch,
            sequence: 1,
        };
        repo.accept_usage_batch(
            AcceptInferenceUsageBatchWrite::new(
                organization_id,
                node_id,
                InferenceUsageBatchV1 {
                    schema: InferenceUsageBatchV1::SCHEMA.into(),
                    gateway_id,
                    batch_id: Uuid::new_v4(),
                    after: None,
                    records: vec![record(cursor, event_id, b"one")],
                },
                Utc::now(),
            )
            .unwrap(),
        )
        .await
        .unwrap();

        let err = repo
            .accept_usage_batch(
                AcceptInferenceUsageBatchWrite::new(
                    organization_id,
                    node_id,
                    InferenceUsageBatchV1 {
                        schema: InferenceUsageBatchV1::SCHEMA.into(),
                        gateway_id,
                        batch_id: Uuid::new_v4(),
                        after: Some(cursor),
                        records: vec![record(
                            InferenceUsageCursorV1 {
                                boot_epoch: epoch,
                                sequence: 2,
                            },
                            event_id,
                            b"other",
                        )],
                    },
                    Utc::now(),
                )
                .unwrap(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, RepositoryError::Conflict(_)));
    }
}
