use crate::modules::inference::domain::{
    apply_inference_usage_batch, project_inserted_usage_records, validate_showback_day_window,
    validate_showback_fact_timestamp, AcceptInferenceUsageBatchWrite, IInferenceUsageRepository,
    InferenceUsageDailyRollup, InferenceUsageDailyRollupKey, InferenceUsageLedgerError,
    InferenceUsageLedgerState, InferenceUsageRequestFact, InferenceUsageRetentionReport,
    InferenceUsageRetentionState, InferenceUsageRetentionSweep,
};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, RepositoryError};
use a3s_cloud_contracts::{InferenceUsageCursorV1, InferenceUsageReceiptV1};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct StoredUsageEvent {
    #[allow(dead_code)]
    digest: String,
    accepted_at: DateTime<Utc>,
    cursor: InferenceUsageCursorV1,
}

#[derive(Debug)]
struct OrganizationUsageState {
    gateways: HashMap<Uuid, InferenceUsageLedgerState>,
    event_meta: HashMap<Uuid, StoredUsageEvent>,
    facts: HashMap<Uuid, InferenceUsageRequestFact>,
    rollups: HashMap<InferenceUsageDailyRollupKey, InferenceUsageDailyRollup>,
    retention: InferenceUsageRetentionState,
}

impl Default for OrganizationUsageState {
    fn default() -> Self {
        Self {
            gateways: HashMap::new(),
            event_meta: HashMap::new(),
            facts: HashMap::new(),
            rollups: HashMap::new(),
            retention: InferenceUsageRetentionState::initial(
                OrganizationId::from_uuid(Uuid::nil()),
            ),
        }
    }
}

/// Process-local Inference usage ledger used by tests and non-Postgres fixtures.
///
/// Production API/worker roles use [`super::PostgresInferenceUsageRepository`].
#[derive(Debug, Default)]
pub struct InMemoryInferenceUsageRepository {
    organizations: Mutex<HashMap<Uuid, OrganizationUsageState>>,
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
        let mut organizations = self
            .organizations
            .lock()
            .map_err(|_| RepositoryError::Storage("inference usage ledger lock poisoned".into()))?;
        let organization = organizations
            .entry(write.organization_id.as_uuid())
            .or_insert_with(|| OrganizationUsageState {
                gateways: HashMap::new(),
                event_meta: HashMap::new(),
                facts: HashMap::new(),
                rollups: HashMap::new(),
                retention: InferenceUsageRetentionState::initial(write.organization_id),
            });
        let state = organization
            .gateways
            .entry(write.batch.gateway_id)
            .or_default();
        let applied =
            apply_inference_usage_batch(state, &write.batch).map_err(|error| match error {
                InferenceUsageLedgerError::Conflict(message) => RepositoryError::Conflict(message),
                InferenceUsageLedgerError::Contract(message) => RepositoryError::Storage(message),
            })?;
        for record in &write.batch.records {
            if applied.inserted_event_ids.contains(&record.event_id) {
                organization.event_meta.insert(
                    record.event_id,
                    StoredUsageEvent {
                        digest: record.payload_sha256.clone(),
                        accepted_at: write.accepted_at,
                        cursor: record.cursor.clone(),
                    },
                );
            }
        }
        *state = applied.next;
        project_inserted_usage_records(
            &mut organization.facts,
            &mut organization.rollups,
            write.batch.gateway_id,
            &write.batch,
            &applied.inserted_event_ids,
        )
        .map_err(RepositoryError::Storage)?;
        Ok(applied.receipt)
    }

    async fn list_daily_rollups(
        &self,
        organization_id: OrganizationId,
        environment_id: EnvironmentId,
        from_day: NaiveDate,
        to_day: NaiveDate,
    ) -> Result<Vec<InferenceUsageDailyRollup>, RepositoryError> {
        let organizations = self
            .organizations
            .lock()
            .map_err(|_| RepositoryError::Storage("inference usage ledger lock poisoned".into()))?;
        let available_from = organizations
            .get(&organization_id.as_uuid())
            .and_then(|organization| organization.retention.records_available_from);
        validate_showback_day_window(available_from, from_day, to_day)
            .map_err(RepositoryError::Conflict)?;
        let Some(organization) = organizations.get(&organization_id.as_uuid()) else {
            return Ok(Vec::new());
        };
        let environment_uuid = environment_id.as_uuid();
        let mut rows: Vec<_> = organization
            .rollups
            .values()
            .filter(|rollup| {
                rollup.key.environment_id == environment_uuid
                    && rollup.key.day >= from_day
                    && rollup.key.day <= to_day
            })
            .cloned()
            .collect();
        rows.sort_by(|left, right| {
            (
                left.key.day,
                left.key.environment_id,
                left.key.model_id,
                format!("{:?}", left.key.endpoint),
            )
                .cmp(&(
                    right.key.day,
                    right.key.environment_id,
                    right.key.model_id,
                    format!("{:?}", right.key.endpoint),
                ))
        });
        Ok(rows)
    }

    async fn get_request_fact(
        &self,
        organization_id: OrganizationId,
        request_id: Uuid,
    ) -> Result<Option<InferenceUsageRequestFact>, RepositoryError> {
        let organizations = self
            .organizations
            .lock()
            .map_err(|_| RepositoryError::Storage("inference usage ledger lock poisoned".into()))?;
        let Some(organization) = organizations.get(&organization_id.as_uuid()) else {
            return Ok(None);
        };
        let Some(fact) = organization.facts.get(&request_id).cloned() else {
            return Ok(None);
        };
        if let Err(error) = validate_showback_fact_timestamp(
            organization.retention.records_available_from,
            fact.started_at,
        ) {
            return Err(RepositoryError::Conflict(error));
        }
        Ok(Some(fact))
    }

    async fn retention_available_from(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<DateTime<Utc>>, RepositoryError> {
        Ok(self
            .retention_state(organization_id)
            .await?
            .records_available_from)
    }

    async fn retention_state(
        &self,
        organization_id: OrganizationId,
    ) -> Result<InferenceUsageRetentionState, RepositoryError> {
        let organizations = self
            .organizations
            .lock()
            .map_err(|_| RepositoryError::Storage("inference usage ledger lock poisoned".into()))?;
        Ok(organizations
            .get(&organization_id.as_uuid())
            .map(|organization| organization.retention.clone())
            .unwrap_or_else(|| InferenceUsageRetentionState::initial(organization_id)))
    }

    async fn sweep_retention(
        &self,
        sweep: InferenceUsageRetentionSweep,
    ) -> Result<InferenceUsageRetentionReport, RepositoryError> {
        sweep.validate().map_err(RepositoryError::Storage)?;
        let mut organizations = self
            .organizations
            .lock()
            .map_err(|_| RepositoryError::Storage("inference usage ledger lock poisoned".into()))?;
        let mut due: Vec<_> = organizations
            .iter()
            .filter(|(_, state)| state.retention.next_scan_at <= sweep.swept_at)
            .map(|(organization_id, state)| (state.retention.next_scan_at, *organization_id))
            .collect();
        due.sort_unstable();
        due.truncate(sweep.organization_batch_size);

        let mut report = InferenceUsageRetentionReport::default();
        let mut remaining = sweep.record_batch_size;
        for (_, organization_uuid) in due {
            if remaining == 0 {
                break;
            }
            report.inspected_organizations += 1;
            let organization = organizations
                .get_mut(&organization_uuid)
                .expect("selected retention organization");
            let current_boundary = organization.retention.records_available_from;
            let boundary =
                current_boundary.map_or(sweep.cutoff, |current| current.max(sweep.cutoff));

            let mut purge_event_ids = organization
                .event_meta
                .iter()
                .filter_map(|(event_id, meta)| {
                    if meta.accepted_at >= boundary {
                        return None;
                    }
                    let Some(ledger) = organization
                        .gateways
                        .values()
                        .find(|ledger| ledger.events.contains_key(event_id))
                    else {
                        return Some(*event_id);
                    };
                    let Some(watermark) = ledger.watermark else {
                        return None;
                    };
                    if meta.cursor.boot_epoch != watermark.boot_epoch
                        || meta.cursor.sequence <= watermark.sequence
                    {
                        Some(*event_id)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            purge_event_ids.sort_unstable();
            purge_event_ids.truncate(remaining);
            let purge_set: BTreeSet<_> = purge_event_ids.iter().copied().collect();

            for event_id in &purge_set {
                organization.event_meta.remove(event_id);
                for ledger in organization.gateways.values_mut() {
                    ledger.events.remove(event_id);
                }
            }
            remaining -= purge_set.len();
            report.deleted_records += purge_set.len();

            let before_facts = organization.facts.len();
            organization
                .facts
                .retain(|_, fact| fact.started_at >= boundary);
            let deleted_facts = before_facts - organization.facts.len();
            report.deleted_records += deleted_facts;
            remaining = remaining.saturating_sub(deleted_facts.min(remaining));

            let boundary_day = boundary.date_naive();
            let before_rollups = organization.rollups.len();
            organization
                .rollups
                .retain(|key, _| key.day >= boundary_day);
            let deleted_rollups = before_rollups - organization.rollups.len();
            report.deleted_records += deleted_rollups;
            remaining = remaining.saturating_sub(deleted_rollups.min(remaining));

            let events_remaining = organization
                .event_meta
                .values()
                .any(|meta| meta.accepted_at < boundary);
            let facts_remaining = organization
                .facts
                .values()
                .any(|fact| fact.started_at < boundary);
            let rollups_remaining = organization
                .rollups
                .keys()
                .any(|key| key.day < boundary_day);
            let completed = !events_remaining && !facts_remaining && !rollups_remaining;

            let state = &mut organization.retention;
            let total_deleted_records = state
                .total_deleted_records
                .checked_add((purge_set.len() + deleted_facts + deleted_rollups) as u64)
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "inference usage retention deleted-record count overflowed".into(),
                    )
                })?;
            let version = state.version.checked_add(1).ok_or_else(|| {
                RepositoryError::Storage("inference usage retention version overflowed".into())
            })?;
            state.records_available_from = Some(boundary);
            if completed {
                state.records_deleted_before = Some(boundary);
                state.last_completed_at = Some(sweep.swept_at);
                report.completed_organizations += 1;
            }
            state.applied_policy_digest = Some(sweep.policy_digest.clone());
            state.total_deleted_records = total_deleted_records;
            state.last_swept_at = Some(sweep.swept_at);
            state.next_scan_at = sweep.next_scan_at;
            state.version = version;
            state.validate().map_err(RepositoryError::Storage)?;
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{EnvironmentId, NodeId};
    use a3s_cloud_contracts::{
        InferenceUsageBatchV1, InferenceUsageCursorV1, InferenceUsageEndpointV1,
        InferenceUsageLifecycleEventV1, InferenceUsageLifecycleKindV1,
        InferenceUsageMeasurementCompletenessV1, InferenceUsageRecordV1,
        InferenceUsageRequestEvidenceV1, InferenceUsageTerminalOutcomeV1,
    };
    use base64::Engine;
    use chrono::{DateTime, Utc};
    use sha2::{Digest, Sha256};

    fn request_evidence() -> InferenceUsageRequestEvidenceV1 {
        InferenceUsageRequestEvidenceV1 {
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
        }
    }

    fn lifecycle(kind: InferenceUsageLifecycleKindV1, at: &str, terminal: bool) -> Vec<u8> {
        let event = InferenceUsageLifecycleEventV1 {
            schema: InferenceUsageLifecycleEventV1::SCHEMA.into(),
            kind,
            occurred_at: DateTime::parse_from_rfc3339(at)
                .unwrap()
                .with_timezone(&Utc),
            request: request_evidence(),
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
    async fn accepts_contiguous_batch_and_refuses_wrong_after() {
        let repo = InMemoryInferenceUsageRepository::new();
        let organization_id = OrganizationId::from_uuid(Uuid::from_u128(9));
        let node_id = NodeId::from_uuid(Uuid::from_u128(8));
        let gateway_id = Uuid::from_u128(2);
        let epoch = Uuid::from_u128(1);
        let payload = lifecycle(
            InferenceUsageLifecycleKindV1::RequestStarted,
            "2026-01-02T10:00:00Z",
            false,
        );
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
                &payload,
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
        let first_payload = lifecycle(
            InferenceUsageLifecycleKindV1::RequestStarted,
            "2026-01-02T10:00:00Z",
            false,
        );
        repo.accept_usage_batch(
            AcceptInferenceUsageBatchWrite::new(
                organization_id,
                node_id,
                InferenceUsageBatchV1 {
                    schema: InferenceUsageBatchV1::SCHEMA.into(),
                    gateway_id,
                    batch_id: Uuid::new_v4(),
                    after: None,
                    records: vec![record(cursor, event_id, &first_payload)],
                },
                Utc::now(),
            )
            .unwrap(),
        )
        .await
        .unwrap();

        let mut other = serde_json::from_slice::<serde_json::Value>(&first_payload).unwrap();
        other["request"]["model_alias"] = serde_json::json!("other-alias");
        let other_payload = serde_json::to_vec(&other).unwrap();
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
                            &other_payload,
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

    #[tokio::test]
    async fn projects_terminal_request_into_daily_rollup_once() {
        let repo = InMemoryInferenceUsageRepository::new();
        let organization_id = OrganizationId::from_uuid(Uuid::from_u128(9));
        let node_id = NodeId::from_uuid(Uuid::from_u128(8));
        let gateway_id = Uuid::from_u128(2);
        let epoch = Uuid::from_u128(1);
        let started = lifecycle(
            InferenceUsageLifecycleKindV1::RequestStarted,
            "2026-01-02T10:00:00Z",
            false,
        );
        let finished = lifecycle(
            InferenceUsageLifecycleKindV1::RequestTerminal,
            "2026-01-02T10:00:01Z",
            true,
        );
        repo.accept_usage_batch(
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
                            &started,
                        ),
                        record(
                            InferenceUsageCursorV1 {
                                boot_epoch: epoch,
                                sequence: 2,
                            },
                            Uuid::from_u128(11),
                            &finished,
                        ),
                    ],
                },
                Utc::now(),
            )
            .unwrap(),
        )
        .await
        .unwrap();

        let fact = repo
            .get_request_fact(organization_id, Uuid::from_u128(100))
            .await
            .unwrap()
            .expect("request fact");
        assert!(fact.is_terminal());
        assert_eq!(fact.total_tokens, Some(11));

        let day = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
        let environment_id = EnvironmentId::from_uuid(Uuid::from_u128(101));
        let rollups = repo
            .list_daily_rollups(organization_id, environment_id, day, day)
            .await
            .unwrap();
        assert_eq!(rollups.len(), 1);
        assert_eq!(rollups[0].request_count, 1);
        assert_eq!(rollups[0].succeeded_count, 1);
        assert_eq!(rollups[0].total_tokens, 11);
        assert!(repo
            .list_daily_rollups(
                organization_id,
                EnvironmentId::from_uuid(Uuid::from_u128(999)),
                day,
                day,
            )
            .await
            .unwrap()
            .is_empty());

        // Exact redelivery of the same contiguous batch must not double-count.
        repo.accept_usage_batch(
            AcceptInferenceUsageBatchWrite::new(
                organization_id,
                node_id,
                InferenceUsageBatchV1 {
                    schema: InferenceUsageBatchV1::SCHEMA.into(),
                    gateway_id,
                    batch_id: Uuid::from_u128(4),
                    after: None,
                    records: vec![
                        record(
                            InferenceUsageCursorV1 {
                                boot_epoch: epoch,
                                sequence: 1,
                            },
                            Uuid::from_u128(10),
                            &started,
                        ),
                        record(
                            InferenceUsageCursorV1 {
                                boot_epoch: epoch,
                                sequence: 2,
                            },
                            Uuid::from_u128(11),
                            &finished,
                        ),
                    ],
                },
                Utc::now(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
        let rollups = repo
            .list_daily_rollups(organization_id, environment_id, day, day)
            .await
            .unwrap();
        assert_eq!(rollups.len(), 1);
        assert_eq!(rollups[0].request_count, 1);
    }
}
