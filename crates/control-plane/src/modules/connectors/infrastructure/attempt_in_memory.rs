use crate::modules::connectors::domain::{
    reservation_record, BeginConnectorExecutionDispatch, ConnectorExecutionAttemptCursor,
    ConnectorExecutionAttemptRecord, ConnectorExecutionAttemptState, ConnectorExecutionEvidence,
    ConnectorExecutionEvidenceCursor, ConnectorExecutionOutcome, ConnectorExecutionReservation,
    IConnectorExecutionAttemptRepository, IConnectorExecutionEvidenceRepository,
    ReserveConnectorExecutionAttempt, SettleConnectorExecutionAttempt,
    MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE, MAXIMUM_CONNECTOR_EXECUTION_EVIDENCE_PAGE_SIZE,
};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotentWrite, OrganizationId,
    ProjectId, RepositoryError,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;
use uuid::Uuid;

type AttemptKey = (
    OrganizationId,
    ProjectId,
    EnvironmentId,
    ConnectorProfileId,
    ConnectorRevisionId,
    Uuid,
);

/// One in-memory implementation of both durable attempt control and evidence reads.
/// Evidence can only enter this store through the atomic `settle` transition.
#[derive(Default)]
pub struct InMemoryConnectorExecutionRepository {
    attempts: RwLock<BTreeMap<AttemptKey, ConnectorExecutionAttemptRecord>>,
}

impl InMemoryConnectorExecutionRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IConnectorExecutionAttemptRepository for InMemoryConnectorExecutionRepository {
    async fn reserve(
        &self,
        request: ReserveConnectorExecutionAttempt,
    ) -> Result<ConnectorExecutionReservation, RepositoryError> {
        request.validate().map_err(RepositoryError::Storage)?;
        let key = binding_key(&request.binding);
        let mut stored = self.attempts.write().await;
        let Some(current) = stored.get(&key).cloned() else {
            let record = reservation_record(&request, 1, request.reserved_at)
                .map_err(RepositoryError::Storage)?;
            let fence = record.attempt.fence();
            stored.insert(key, record);
            return Ok(ConnectorExecutionReservation::Acquired {
                fence,
                replayed: false,
            });
        };
        if current.attempt.binding() != &request.binding {
            return Err(request_conflict());
        }
        match current.attempt.state() {
            ConnectorExecutionAttemptState::Terminal => {
                Ok(ConnectorExecutionReservation::Completed(current))
            }
            ConnectorExecutionAttemptState::Dispatching => {
                match current.attempt.recovery_state(request.reserved_at) {
                    crate::modules::connectors::domain::ConnectorExecutionRecoveryState::InFlight => {
                        Ok(ConnectorExecutionReservation::InFlight(current))
                    }
                    crate::modules::connectors::domain::ConnectorExecutionRecoveryState::Indeterminate => {
                        Ok(ConnectorExecutionReservation::Indeterminate(current))
                    }
                    _ => Err(RepositoryError::Storage(
                        "stored Connector dispatch recovery state is invalid".into(),
                    )),
                }
            }
            ConnectorExecutionAttemptState::Reserved => {
                let current_fence = current.attempt.fence();
                if current_fence.token() == request.fence_token {
                    if current_fence.reserved_at() == request.reserved_at
                        && current_fence.lease_expires_at() == request.lease_expires_at
                        && request.reserved_at < current_fence.lease_expires_at()
                    {
                        return Ok(ConnectorExecutionReservation::Acquired {
                            fence: current_fence,
                            replayed: true,
                        });
                    }
                    return Err(fence_conflict());
                }
                if request.reserved_at < current.attempt.lease_expires_at() {
                    return Ok(ConnectorExecutionReservation::Busy(current));
                }
                let generation = current
                    .attempt
                    .fence_generation()
                    .checked_add(1)
                    .ok_or_else(|| {
                        RepositoryError::Conflict(
                            "Connector execution fence generation overflowed".into(),
                        )
                    })?;
                let record = reservation_record(&request, generation, current.attempt.created_at())
                    .map_err(RepositoryError::Storage)?;
                let fence = record.attempt.fence();
                stored.insert(key, record);
                Ok(ConnectorExecutionReservation::Acquired {
                    fence,
                    replayed: false,
                })
            }
        }
    }

    async fn begin_dispatch(
        &self,
        request: BeginConnectorExecutionDispatch,
    ) -> Result<ConnectorExecutionAttemptRecord, RepositoryError> {
        request.validate().map_err(RepositoryError::Conflict)?;
        let key = binding_key(request.fence.binding());
        let mut stored = self.attempts.write().await;
        let current = stored.get(&key).cloned().ok_or(RepositoryError::NotFound)?;
        if current.attempt.state() != ConnectorExecutionAttemptState::Reserved
            || current.attempt.fence() != request.fence
        {
            return Err(fence_conflict());
        }
        let attempt = crate::modules::connectors::domain::ConnectorExecutionAttempt::restore(
            current.attempt.binding().clone(),
            ConnectorExecutionAttemptState::Dispatching,
            current.attempt.fence_generation(),
            request.fence.token(),
            current.attempt.reserved_at(),
            current.attempt.lease_expires_at(),
            Some(request.dispatch_started_at),
            Some(request.outcome_deadline_at),
            None,
            current.attempt.created_at(),
        )
        .map_err(RepositoryError::Conflict)?;
        let record = ConnectorExecutionAttemptRecord::new(attempt, None)
            .map_err(RepositoryError::Storage)?;
        stored.insert(key, record.clone());
        Ok(record)
    }

    async fn settle(
        &self,
        request: SettleConnectorExecutionAttempt,
    ) -> Result<IdempotentWrite<ConnectorExecutionAttemptRecord>, RepositoryError> {
        request.validate().map_err(RepositoryError::Conflict)?;
        let key = binding_key(request.fence.binding());
        let mut stored = self.attempts.write().await;
        let current = stored.get(&key).cloned().ok_or(RepositoryError::NotFound)?;
        if current.attempt.state() == ConnectorExecutionAttemptState::Terminal {
            if current.attempt.fence() == request.fence
                && current.evidence.as_ref() == Some(&request.evidence)
            {
                return Ok(IdempotentWrite {
                    value: current,
                    replayed: true,
                });
            }
            return Err(evidence_conflict());
        }
        if current.attempt.fence() != request.fence {
            return Err(fence_conflict());
        }
        let (dispatch_started_at, outcome_deadline_at) = match current.attempt.state() {
            ConnectorExecutionAttemptState::Reserved => {
                if request.evidence.outcome() == ConnectorExecutionOutcome::Accepted
                    || request.evidence.response_status().is_some()
                    || request.evidence.started_at() != current.attempt.reserved_at()
                    || request.evidence.completed_at() > current.attempt.lease_expires_at()
                {
                    return Err(RepositoryError::Conflict(
                        "Connector pre-dispatch settlement claims a provider response".into(),
                    ));
                }
                (None, None)
            }
            ConnectorExecutionAttemptState::Dispatching => {
                if request.evidence.started_at()
                    != current
                        .attempt
                        .dispatch_started_at()
                        .expect("validated dispatch")
                {
                    return Err(RepositoryError::Conflict(
                        "Connector execution evidence does not match dispatch start".into(),
                    ));
                }
                (
                    current.attempt.dispatch_started_at(),
                    current.attempt.outcome_deadline_at(),
                )
            }
            ConnectorExecutionAttemptState::Terminal => unreachable!("handled above"),
        };
        let attempt = crate::modules::connectors::domain::ConnectorExecutionAttempt::restore(
            current.attempt.binding().clone(),
            ConnectorExecutionAttemptState::Terminal,
            current.attempt.fence_generation(),
            request.fence.token(),
            current.attempt.reserved_at(),
            current.attempt.lease_expires_at(),
            dispatch_started_at,
            outcome_deadline_at,
            Some(request.evidence.completed_at()),
            current.attempt.created_at(),
        )
        .map_err(RepositoryError::Conflict)?;
        let record = ConnectorExecutionAttemptRecord::new(attempt, Some(request.evidence))
            .map_err(RepositoryError::Storage)?;
        stored.insert(key, record.clone());
        Ok(IdempotentWrite {
            value: record,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        attempt_id: Uuid,
    ) -> Result<Option<ConnectorExecutionAttemptRecord>, RepositoryError> {
        Ok(self
            .attempts
            .read()
            .await
            .get(&(
                organization_id,
                project_id,
                environment_id,
                profile_id,
                revision_id,
                attempt_id,
            ))
            .cloned())
    }

    async fn list_unresolved_page(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        after: Option<ConnectorExecutionAttemptCursor>,
        limit: usize,
    ) -> Result<Vec<ConnectorExecutionAttemptRecord>, RepositoryError> {
        let after = after
            .map(ConnectorExecutionAttemptCursor::validate)
            .transpose()
            .map_err(RepositoryError::Storage)?;
        let mut records = self
            .attempts
            .read()
            .await
            .values()
            .filter(|record| {
                let binding = record.attempt.binding();
                record.attempt.state() != ConnectorExecutionAttemptState::Terminal
                    && binding.organization_id() == organization_id
                    && binding.project_id() == project_id
                    && binding.environment_id() == environment_id
                    && binding.profile_id() == profile_id
                    && binding.revision_id() == revision_id
                    && after.is_none_or(|cursor| {
                        record.attempt.created_at() < cursor.created_at
                            || record.attempt.created_at() == cursor.created_at
                                && binding.attempt_id() < cursor.attempt_id
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            right
                .attempt
                .created_at()
                .cmp(&left.attempt.created_at())
                .then_with(|| {
                    right
                        .attempt
                        .binding()
                        .attempt_id()
                        .cmp(&left.attempt.binding().attempt_id())
                })
        });
        records.truncate(limit.clamp(1, MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE + 1));
        Ok(records)
    }
}

#[async_trait]
impl IConnectorExecutionEvidenceRepository for InMemoryConnectorExecutionRepository {
    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        attempt_id: Uuid,
    ) -> Result<Option<ConnectorExecutionEvidence>, RepositoryError> {
        Ok(self
            .attempts
            .read()
            .await
            .get(&(
                organization_id,
                project_id,
                environment_id,
                profile_id,
                revision_id,
                attempt_id,
            ))
            .and_then(|record| record.evidence.clone()))
    }

    async fn list_page(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        after: Option<ConnectorExecutionEvidenceCursor>,
        limit: usize,
    ) -> Result<Vec<ConnectorExecutionEvidence>, RepositoryError> {
        let after = after
            .map(ConnectorExecutionEvidenceCursor::validate)
            .transpose()
            .map_err(RepositoryError::Storage)?;
        let mut evidence = self
            .attempts
            .read()
            .await
            .values()
            .filter_map(|record| record.evidence.as_ref())
            .filter(|value| {
                value.organization_id() == organization_id
                    && value.project_id() == project_id
                    && value.environment_id() == environment_id
                    && value.profile_id() == profile_id
                    && value.revision_id() == revision_id
                    && after.is_none_or(|cursor| {
                        value.completed_at() < cursor.completed_at
                            || value.completed_at() == cursor.completed_at
                                && value.attempt_id() < cursor.attempt_id
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        evidence.sort_by(|left, right| {
            right
                .completed_at()
                .cmp(&left.completed_at())
                .then_with(|| right.attempt_id().cmp(&left.attempt_id()))
        });
        evidence.truncate(limit.clamp(1, MAXIMUM_CONNECTOR_EXECUTION_EVIDENCE_PAGE_SIZE + 1));
        Ok(evidence)
    }
}

fn binding_key(
    binding: &crate::modules::connectors::domain::ConnectorExecutionAttemptBinding,
) -> AttemptKey {
    (
        binding.organization_id(),
        binding.project_id(),
        binding.environment_id(),
        binding.profile_id(),
        binding.revision_id(),
        binding.attempt_id(),
    )
}

fn request_conflict() -> RepositoryError {
    RepositoryError::Conflict(
        "Connector execution attempt identity is already bound to another request".into(),
    )
}

fn fence_conflict() -> RepositoryError {
    RepositoryError::Conflict("Connector execution fence is stale or ambiguous".into())
}

fn evidence_conflict() -> RepositoryError {
    RepositoryError::Conflict(
        "Connector execution attempt already records a different terminal fact".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::connectors::domain::{
        BeginConnectorExecutionDispatch, ConnectorDefinition, ConnectorExecutionAttemptBinding,
        ConnectorExecutionReceipt, ConnectorExecutionRequest, ConnectorHttpAuthentication,
        ConnectorHttpDefinition, ConnectorHttpDefinitionSpec, ConnectorHttpDestination,
        ConnectorHttpMethod, ConnectorHttpStatusPolicy, ConnectorRevision,
        ReserveConnectorExecutionAttempt, SettleConnectorExecutionAttempt,
    };
    use crate::modules::shared_kernel::domain::{canonical_timestamp, PrincipalId};
    use chrono::{DateTime, Duration, Utc};
    use std::sync::Arc;

    fn exact(now: DateTime<Utc>) -> (ConnectorRevision, ConnectorExecutionRequest) {
        let revision = ConnectorRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
            ConnectorDefinition::Http(
                ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
                    destination: ConnectorHttpDestination::LiteralHttps {
                        endpoint: "https://hooks.example.test/attempt".into(),
                    },
                    method: ConnectorHttpMethod::Post,
                    request_content_type: "application/json".into(),
                    maximum_request_bytes: 1024,
                    maximum_response_bytes: 1024,
                    timeout_milliseconds: 5_000,
                    status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
                    authentication: ConnectorHttpAuthentication::None,
                })
                .expect("definition"),
            ),
            PrincipalId::new(),
            now,
        )
        .expect("revision");
        let request = ConnectorExecutionRequest::new(
            revision.id,
            Uuid::now_v7(),
            "application/json",
            b"request".to_vec(),
        )
        .expect("request");
        (revision, request)
    }

    fn reservation(
        revision: &ConnectorRevision,
        request: &ConnectorExecutionRequest,
        token: Uuid,
        now: DateTime<Utc>,
    ) -> ReserveConnectorExecutionAttempt {
        ReserveConnectorExecutionAttempt::new(
            ConnectorExecutionAttemptBinding::from_exact(revision, request).expect("binding"),
            token,
            now,
            now + Duration::seconds(30),
        )
        .expect("reservation")
    }

    #[tokio::test]
    async fn concurrent_reservation_has_one_fence_and_exact_replay() {
        let repository = Arc::new(InMemoryConnectorExecutionRepository::new());
        let now = canonical_timestamp(Utc::now());
        let (revision, request) = exact(now);
        let left = reservation(&revision, &request, Uuid::now_v7(), now);
        let right = reservation(&revision, &request, Uuid::now_v7(), now);
        let (left, right) = tokio::join!(repository.reserve(left), repository.reserve(right));
        let outcomes = [left.expect("left"), right.expect("right")];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, ConnectorExecutionReservation::Acquired { .. }))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, ConnectorExecutionReservation::Busy(_)))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn expired_reservation_rotates_generation_but_dispatch_never_reacquires() {
        let repository = InMemoryConnectorExecutionRepository::new();
        let now = canonical_timestamp(Utc::now());
        let (revision, request) = exact(now);
        let first = reservation(&revision, &request, Uuid::now_v7(), now);
        let first_fence = match repository.reserve(first).await.expect("first") {
            ConnectorExecutionReservation::Acquired { fence, .. } => fence,
            other => panic!("unexpected reservation: {other:?}"),
        };
        let takeover_at = first_fence.lease_expires_at();
        let second = reservation(&revision, &request, Uuid::now_v7(), takeover_at);
        let second_fence = match repository.reserve(second).await.expect("takeover") {
            ConnectorExecutionReservation::Acquired { fence, .. } => fence,
            other => panic!("unexpected takeover: {other:?}"),
        };
        assert_eq!(second_fence.generation(), 2);
        assert!(repository
            .begin_dispatch(
                BeginConnectorExecutionDispatch::new(
                    first_fence,
                    now + Duration::seconds(1),
                    now + Duration::seconds(10),
                )
                .expect("stale transition"),
            )
            .await
            .is_err());

        let dispatch_started = takeover_at + Duration::seconds(1);
        let deadline = dispatch_started + Duration::seconds(5);
        repository
            .begin_dispatch(
                BeginConnectorExecutionDispatch::new(
                    second_fence.clone(),
                    dispatch_started,
                    deadline,
                )
                .expect("dispatch"),
            )
            .await
            .expect("begin dispatch");
        let retry = reservation(&revision, &request, Uuid::now_v7(), deadline);
        assert!(matches!(
            repository.reserve(retry).await.expect("recover"),
            ConnectorExecutionReservation::Indeterminate(_)
        ));
        assert!(repository
            .begin_dispatch(
                BeginConnectorExecutionDispatch::new(second_fence, dispatch_started, deadline,)
                    .expect("replay transition"),
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn settlement_atomically_exposes_evidence_and_replays_exactly() {
        let repository = InMemoryConnectorExecutionRepository::new();
        let now = canonical_timestamp(Utc::now());
        let (revision, request) = exact(now);
        let fence = match repository
            .reserve(reservation(&revision, &request, Uuid::now_v7(), now))
            .await
            .expect("reserve")
        {
            ConnectorExecutionReservation::Acquired { fence, .. } => fence,
            other => panic!("unexpected reservation: {other:?}"),
        };
        let started = now + Duration::seconds(1);
        repository
            .begin_dispatch(
                BeginConnectorExecutionDispatch::new(
                    fence.clone(),
                    started,
                    started + Duration::seconds(10),
                )
                .expect("dispatch"),
            )
            .await
            .expect("begin dispatch");
        let receipt = ConnectorExecutionReceipt::accepted(
            revision.id,
            request.attempt_id(),
            started + Duration::milliseconds(5),
            202,
            None,
            b"response".to_vec(),
        )
        .expect("receipt");
        let evidence = ConnectorExecutionEvidence::accepted(&revision, &request, &receipt, started)
            .expect("evidence");
        let settlement =
            SettleConnectorExecutionAttempt::new(fence, evidence.clone()).expect("settlement");
        assert!(
            !repository
                .settle(settlement.clone())
                .await
                .expect("settle")
                .replayed
        );
        assert!(
            repository
                .settle(settlement)
                .await
                .expect("settle replay")
                .replayed
        );
        assert_eq!(
            IConnectorExecutionEvidenceRepository::find(
                &repository,
                revision.organization_id,
                revision.project_id,
                revision.environment_id,
                revision.profile_id,
                revision.id,
                request.attempt_id(),
            )
            .await
            .expect("find evidence"),
            Some(evidence)
        );
    }
}
