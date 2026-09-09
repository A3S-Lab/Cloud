use crate::modules::inference::domain::{
    IInferenceUsageRepository, InferenceUsageRetentionPolicy, InferenceUsageRetentionReport,
    InferenceUsageRetentionSweep, MAXIMUM_INFERENCE_USAGE_RETENTION_BATCH_SIZE,
};
use crate::modules::shared_kernel::domain::{canonical_timestamp, RepositoryError};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

pub struct InferenceUsageRetentionWorker {
    repository: Arc<dyn IInferenceUsageRepository>,
    policy: InferenceUsageRetentionPolicy,
    poll_interval: Duration,
    organization_batch_size: usize,
    record_batch_size: usize,
}

impl InferenceUsageRetentionWorker {
    pub fn new(
        repository: Arc<dyn IInferenceUsageRepository>,
        retention: Duration,
        poll_interval: Duration,
        organization_batch_size: usize,
        record_batch_size: usize,
    ) -> Result<Self, String> {
        let policy = InferenceUsageRetentionPolicy::new(retention)?;
        if poll_interval.is_zero()
            || poll_interval > Duration::from_secs(24 * 60 * 60)
            || poll_interval > retention
            || organization_batch_size == 0
            || organization_batch_size > MAXIMUM_INFERENCE_USAGE_RETENTION_BATCH_SIZE
            || record_batch_size == 0
            || record_batch_size > MAXIMUM_INFERENCE_USAGE_RETENTION_BATCH_SIZE
        {
            return Err(
                "inference usage retention requires a bounded poll interval and batches".into(),
            );
        }
        Ok(Self {
            repository,
            policy,
            poll_interval,
            organization_batch_size,
            record_batch_size,
        })
    }

    pub fn policy(&self) -> &InferenceUsageRetentionPolicy {
        &self.policy
    }

    pub async fn run_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<InferenceUsageRetentionReport, RepositoryError> {
        let swept_at = canonical_timestamp(now);
        let cutoff = self
            .policy
            .cutoff(swept_at)
            .map_err(RepositoryError::Storage)?;
        let interval = chrono::Duration::from_std(self.poll_interval).map_err(|_| {
            RepositoryError::Storage("inference usage retention interval overflowed".into())
        })?;
        let next_scan_at = swept_at
            .checked_add_signed(interval)
            .map(canonical_timestamp)
            .ok_or_else(|| {
                RepositoryError::Storage("inference usage retention schedule overflowed".into())
            })?;
        self.repository
            .sweep_retention(InferenceUsageRetentionSweep {
                cutoff,
                swept_at,
                next_scan_at,
                policy_digest: self.policy.digest().clone(),
                organization_batch_size: self.organization_batch_size,
                record_batch_size: self.record_batch_size,
            })
            .await
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.poll_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once(Utc::now()).await {
                        Ok(report) => tracing::debug!(
                            inspected_organizations = report.inspected_organizations,
                            completed_organizations = report.completed_organizations,
                            deleted_records = report.deleted_records,
                            "inference usage retention cycle completed"
                        ),
                        Err(error) => tracing::error!(
                            error = %error,
                            "inference usage retention cycle failed"
                        ),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::inference::domain::AcceptInferenceUsageBatchWrite;
    use crate::modules::inference::InMemoryInferenceUsageRepository;
    use crate::modules::shared_kernel::domain::{EnvironmentId, NodeId, OrganizationId};
    use a3s_cloud_contracts::{
        InferenceUsageBatchV1, InferenceUsageCursorV1, InferenceUsageEndpointV1,
        InferenceUsageLifecycleEventV1, InferenceUsageLifecycleKindV1,
        InferenceUsageMeasurementCompletenessV1, InferenceUsageRecordV1,
        InferenceUsageRequestEvidenceV1, InferenceUsageTerminalOutcomeV1,
    };
    use base64::Engine;
    use chrono::{Duration as ChronoDuration, NaiveDate, TimeZone};
    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    fn request_evidence(request_id: Uuid) -> InferenceUsageRequestEvidenceV1 {
        InferenceUsageRequestEvidenceV1 {
            request_id,
            correlation_id: "corr".into(),
            environment_id: Uuid::from_u128(101),
            credential_id: Uuid::from_u128(102),
            credential_generation: 1,
            route_id: Uuid::from_u128(103),
            route_policy_revision: 1,
            endpoint: InferenceUsageEndpointV1::ChatCompletions,
            model_alias: "alias".into(),
            model_id: Uuid::from_u128(104),
        }
    }

    fn lifecycle_payload(
        kind: InferenceUsageLifecycleKindV1,
        at: DateTime<Utc>,
        request_id: Uuid,
        terminal: bool,
    ) -> Vec<u8> {
        let event = InferenceUsageLifecycleEventV1 {
            schema: InferenceUsageLifecycleEventV1::SCHEMA.into(),
            kind,
            occurred_at: at,
            request: request_evidence(request_id),
            attempt: None,
            outcome: terminal.then_some(InferenceUsageTerminalOutcomeV1::Succeeded),
            http_status: terminal.then_some(200),
            duration_ms: terminal.then_some(5),
            measurement_completeness: terminal
                .then_some(InferenceUsageMeasurementCompletenessV1::UpstreamUsage),
            total_tokens: terminal.then_some(11),
        };
        serde_json::to_vec(&event).unwrap()
    }

    fn record(
        cursor: InferenceUsageCursorV1,
        event_id: Uuid,
        payload: &[u8],
    ) -> InferenceUsageRecordV1 {
        InferenceUsageRecordV1 {
            cursor,
            event_id,
            payload_base64: base64::engine::general_purpose::STANDARD.encode(payload),
            payload_sha256: format!("{:x}", Sha256::digest(payload)),
        }
    }

    #[tokio::test]
    async fn sweep_hides_showback_then_purges_and_conflicts_stale_windows() {
        let repository = Arc::new(InMemoryInferenceUsageRepository::new());
        let organization_id = OrganizationId::from_uuid(Uuid::from_u128(9));
        let node_id = NodeId::from_uuid(Uuid::from_u128(8));
        let gateway_id = Uuid::from_u128(2);
        let epoch = Uuid::from_u128(1);
        let now = Utc
            .with_ymd_and_hms(2026, 8, 23, 12, 0, 0)
            .single()
            .expect("now");
        let old_at = now - ChronoDuration::days(3);
        let recent_at = now - ChronoDuration::hours(1);

        repository
            .accept_usage_batch(
                AcceptInferenceUsageBatchWrite::new(
                    organization_id,
                    node_id,
                    InferenceUsageBatchV1 {
                        schema: InferenceUsageBatchV1::SCHEMA.into(),
                        gateway_id,
                        batch_id: Uuid::from_u128(3),
                        after: None,
                        records: vec![
                            record(
                                InferenceUsageCursorV1 {
                                    boot_epoch: epoch,
                                    sequence: 1,
                                },
                                Uuid::from_u128(10),
                                &lifecycle_payload(
                                    InferenceUsageLifecycleKindV1::RequestStarted,
                                    old_at,
                                    Uuid::from_u128(100),
                                    false,
                                ),
                            ),
                            record(
                                InferenceUsageCursorV1 {
                                    boot_epoch: epoch,
                                    sequence: 2,
                                },
                                Uuid::from_u128(11),
                                &lifecycle_payload(
                                    InferenceUsageLifecycleKindV1::RequestTerminal,
                                    old_at + ChronoDuration::seconds(1),
                                    Uuid::from_u128(100),
                                    true,
                                ),
                            ),
                        ],
                    },
                    old_at,
                )
                .unwrap(),
            )
            .await
            .expect("accept old");
        repository
            .accept_usage_batch(
                AcceptInferenceUsageBatchWrite::new(
                    organization_id,
                    node_id,
                    InferenceUsageBatchV1 {
                        schema: InferenceUsageBatchV1::SCHEMA.into(),
                        gateway_id,
                        batch_id: Uuid::from_u128(4),
                        after: Some(InferenceUsageCursorV1 {
                            boot_epoch: epoch,
                            sequence: 2,
                        }),
                        records: vec![record(
                            InferenceUsageCursorV1 {
                                boot_epoch: epoch,
                                sequence: 3,
                            },
                            Uuid::from_u128(12),
                            &lifecycle_payload(
                                InferenceUsageLifecycleKindV1::RequestStarted,
                                recent_at,
                                Uuid::from_u128(200),
                                false,
                            ),
                        )],
                    },
                    recent_at,
                )
                .unwrap(),
            )
            .await
            .expect("accept recent started");
        repository
            .accept_usage_batch(
                AcceptInferenceUsageBatchWrite::new(
                    organization_id,
                    node_id,
                    InferenceUsageBatchV1 {
                        schema: InferenceUsageBatchV1::SCHEMA.into(),
                        gateway_id,
                        batch_id: Uuid::from_u128(5),
                        after: Some(InferenceUsageCursorV1 {
                            boot_epoch: epoch,
                            sequence: 3,
                        }),
                        records: vec![record(
                            InferenceUsageCursorV1 {
                                boot_epoch: epoch,
                                sequence: 4,
                            },
                            Uuid::from_u128(13),
                            &lifecycle_payload(
                                InferenceUsageLifecycleKindV1::RequestTerminal,
                                recent_at + ChronoDuration::seconds(1),
                                Uuid::from_u128(200),
                                true,
                            ),
                        )],
                    },
                    recent_at,
                )
                .unwrap(),
            )
            .await
            .expect("accept recent terminal");

        let worker = InferenceUsageRetentionWorker::new(
            repository.clone(),
            Duration::from_secs(24 * 60 * 60),
            Duration::from_secs(1),
            1,
            10,
        )
        .expect("worker");
        let report = worker.run_once(now).await.expect("sweep");
        assert_eq!(report.inspected_organizations, 1);
        assert!(report.deleted_records >= 1);
        assert_eq!(report.completed_organizations, 1);

        let boundary = now - ChronoDuration::days(1);
        let available = repository
            .retention_available_from(organization_id)
            .await
            .expect("available");
        assert_eq!(available, Some(boundary));

        let environment_id = EnvironmentId::from_uuid(Uuid::from_u128(101));
        let visible = repository
            .list_daily_rollups(
                organization_id,
                environment_id,
                boundary.date_naive(),
                now.date_naive(),
            )
            .await
            .expect("retained window");
        assert_eq!(visible.len(), 1);

        let stale = repository
            .list_daily_rollups(
                organization_id,
                environment_id,
                NaiveDate::from_ymd_opt(2026, 8, 20).unwrap(),
                now.date_naive(),
            )
            .await;
        assert!(matches!(stale, Err(RepositoryError::Conflict(_))));
    }
}
