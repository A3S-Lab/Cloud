use crate::modules::inference::domain::{
    apply_inference_usage_batch, AcceptInferenceUsageBatchWrite, IInferenceUsageRepository,
    InferenceUsageLedgerError, InferenceUsageLedgerState,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::InferenceUsageReceiptV1;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

/// Process-local Inference usage ledger used by tests and non-Postgres fixtures.
///
/// Production API/worker roles use [`super::PostgresInferenceUsageRepository`].
#[derive(Debug, Default)]
pub struct InMemoryInferenceUsageRepository {
    gateways: Mutex<HashMap<(Uuid, Uuid), InferenceUsageLedgerState>>,
}

impl InMemoryInferenceUsageRepository {
    pub fn new() -> Self {
        Self::default()
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
        let mut gateways = self.gateways.lock().map_err(|_| {
            RepositoryError::Storage("inference usage ledger lock poisoned".into())
        })?;
        let key = (
            write.organization_id.as_uuid(),
            write.batch.gateway_id,
        );
        let state = gateways.entry(key).or_default();
        let applied = apply_inference_usage_batch(state, &write.batch).map_err(|error| {
            match error {
                InferenceUsageLedgerError::Conflict(message) => RepositoryError::Conflict(message),
                InferenceUsageLedgerError::Contract(message) => RepositoryError::Storage(message),
            }
        })?;
        *state = applied.next;
        Ok(applied.receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{NodeId, OrganizationId};
    use a3s_cloud_contracts::{
        InferenceUsageBatchV1, InferenceUsageCursorV1, InferenceUsageRecordV1,
    };
    use base64::Engine;
    use chrono::Utc;
    use sha2::{Digest, Sha256};

    fn record_with_alias(
        cursor: InferenceUsageCursorV1,
        event_id: Uuid,
        model_alias: &str,
    ) -> InferenceUsageRecordV1 {
        use a3s_cloud_contracts::{
            InferenceUsageEndpointV1, InferenceUsageLifecycleEventV1, InferenceUsageLifecycleKindV1,
            InferenceUsageRequestEvidenceV1,
        };
        use chrono::{DateTime, Utc};
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
                model_alias: model_alias.into(),
                model_id: Uuid::from_u128(104),
            },
            attempt: None,
            outcome: None,
            http_status: None,
            duration_ms: None,
            measurement_completeness: None,
            total_tokens: None,
        };
        let payload = serde_json::to_vec(&event).unwrap();
        InferenceUsageRecordV1 {
            cursor,
            event_id,
            payload_base64: base64::engine::general_purpose::STANDARD.encode(&payload),
            payload_sha256: format!("{:x}", Sha256::digest(&payload)),
        }
    }

    fn record(cursor: InferenceUsageCursorV1, event_id: Uuid) -> InferenceUsageRecordV1 {
        record_with_alias(cursor, event_id, "alias")
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
                    records: vec![record(cursor, event_id)],
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
                        records: vec![record_with_alias(
                            InferenceUsageCursorV1 {
                                boot_epoch: epoch,
                                sequence: 2,
                            },
                            event_id,
                            "other-alias",
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
