use super::resource_access::{attempt_not_found, environment, revision_not_found};
use crate::modules::connectors::domain::{
    BeginConnectorExecutionDispatch, ConnectorExecutionAttemptBinding,
    ConnectorExecutionAttemptRecord, ConnectorExecutionEvidence, ConnectorExecutionFence,
    ConnectorExecutionReceipt, ConnectorExecutionRequest, ConnectorExecutionReservation,
    IConnectorExecutionAttemptRepository, IConnectorExecutionPreparationPort,
    IConnectorProfileRepository, ReserveConnectorExecutionAttempt, SettleConnectorExecutionAttempt,
    MAXIMUM_CONNECTOR_EXECUTION_OUTCOME_SECONDS, MAXIMUM_CONNECTOR_EXECUTION_RESERVATION_SECONDS,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId,
    ProjectId, RepositoryError,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub struct ConnectorExecutionServiceOptions {
    pub reservation_lease: Duration,
    pub outcome_grace: Duration,
}

impl Default for ConnectorExecutionServiceOptions {
    fn default() -> Self {
        Self {
            reservation_lease: Duration::from_secs(30),
            outcome_grace: Duration::from_secs(10),
        }
    }
}

impl ConnectorExecutionServiceOptions {
    pub fn validate(self) -> Result<Self, String> {
        if self.reservation_lease.is_zero()
            || self.reservation_lease
                > Duration::from_secs(MAXIMUM_CONNECTOR_EXECUTION_RESERVATION_SECONDS as u64)
            || self.outcome_grace
                > Duration::from_secs(MAXIMUM_CONNECTOR_EXECUTION_OUTCOME_SECONDS as u64)
        {
            return Err("Connector execution service timing options are invalid".into());
        }
        Ok(self)
    }
}

#[derive(Clone)]
pub struct ExecuteConnectorAttempt {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
    pub request: ConnectorExecutionRequest,
    pub resource_access: ResourceAccessEvaluator,
    /// Caller-generated and stable for an ambiguous reservation call only.
    /// It is not a provider idempotency key and changes after lease expiry.
    pub fence_token: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl fmt::Debug for ExecuteConnectorAttempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExecuteConnectorAttempt")
            .field("organization_id", &self.organization_id)
            .field("project_id", &self.project_id)
            .field("environment_id", &self.environment_id)
            .field("profile_id", &self.profile_id)
            .field("revision_id", &self.revision_id)
            .field("request", &self.request)
            .field("fence_token", &"redacted")
            .field("requested_at", &self.requested_at)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectorExecutionAttemptResult {
    Completed {
        evidence: ConnectorExecutionEvidence,
        /// Present only for the process that observed the accepted response.
        /// Replays intentionally return digest-only durable evidence.
        receipt: Option<ConnectorExecutionReceipt>,
        replayed: bool,
    },
    Reserved {
        lease_expires_at: DateTime<Utc>,
    },
    ReservationExpired {
        lease_expires_at: DateTime<Utc>,
    },
    InFlight {
        dispatch_started_at: DateTime<Utc>,
        outcome_deadline_at: DateTime<Utc>,
    },
    Indeterminate {
        dispatch_started_at: DateTime<Utc>,
        outcome_deadline_at: DateTime<Utc>,
    },
    /// The provider call returned, but its evidence transaction did not commit.
    /// Persist this value through `settle_known`; never execute the provider again.
    SettlementPending {
        settlement: SettleConnectorExecutionAttempt,
        receipt: Option<ConnectorExecutionReceipt>,
    },
}

pub struct ConnectorExecutionApplicationService {
    profiles: Arc<dyn IConnectorProfileRepository>,
    attempts: Arc<dyn IConnectorExecutionAttemptRepository>,
    preparation: Arc<dyn IConnectorExecutionPreparationPort>,
    options: ConnectorExecutionServiceOptions,
}

impl ConnectorExecutionApplicationService {
    pub fn new(
        profiles: Arc<dyn IConnectorProfileRepository>,
        attempts: Arc<dyn IConnectorExecutionAttemptRepository>,
        preparation: Arc<dyn IConnectorExecutionPreparationPort>,
        options: ConnectorExecutionServiceOptions,
    ) -> Result<Self, String> {
        Ok(Self {
            profiles,
            attempts,
            preparation,
            options: options.validate()?,
        })
    }

    pub async fn execute(
        &self,
        command: ExecuteConnectorAttempt,
    ) -> ApplicationResult<ConnectorExecutionAttemptResult> {
        validate_command(&command)?;
        environment(
            command.project_id,
            command.environment_id,
            &command.resource_access,
        )?;
        let revision = self
            .profiles
            .find_revision(
                command.organization_id,
                command.project_id,
                command.environment_id,
                command.profile_id,
                command.revision_id,
            )
            .await
            .map_err(map_revision_repository_error)?
            .ok_or_else(revision_not_found)?;
        let binding = ConnectorExecutionAttemptBinding::from_exact(&revision, &command.request)
            .map_err(ApplicationError::Invalid)?;
        let lease_expires_at = add_std(command.requested_at, self.options.reservation_lease)?;
        let reservation = self
            .attempts
            .reserve(
                ReserveConnectorExecutionAttempt::new(
                    binding,
                    command.fence_token,
                    command.requested_at,
                    lease_expires_at,
                )
                .map_err(ApplicationError::Invalid)?,
            )
            .await
            .map_err(map_attempt_repository_error)?;
        let fence = match reservation {
            ConnectorExecutionReservation::Acquired { fence, .. } => fence,
            ConnectorExecutionReservation::Busy(record) => {
                return Ok(ConnectorExecutionAttemptResult::Reserved {
                    lease_expires_at: record.attempt.lease_expires_at(),
                })
            }
            ConnectorExecutionReservation::InFlight(record) => return in_flight_result(&record),
            ConnectorExecutionReservation::Indeterminate(record) => {
                return indeterminate_result(&record)
            }
            ConnectorExecutionReservation::Completed(record) => {
                return completed_from_record(record, true)
            }
        };

        let prepared = match self.preparation.prepare(&revision, &command.request).await {
            Ok(prepared) => prepared,
            Err(error) => {
                let completed_at = canonical_timestamp(Utc::now()).max(fence.reserved_at());
                if completed_at > fence.lease_expires_at() {
                    return Ok(ConnectorExecutionAttemptResult::ReservationExpired {
                        lease_expires_at: fence.lease_expires_at(),
                    });
                }
                let evidence = error_evidence(
                    &revision,
                    &command.request,
                    error,
                    fence.reserved_at(),
                    completed_at,
                )?;
                return self.settle_preflight(fence, evidence).await;
            }
        };

        let dispatch_started_at = canonical_timestamp(Utc::now()).max(fence.reserved_at());
        if dispatch_started_at >= fence.lease_expires_at() {
            return Ok(ConnectorExecutionAttemptResult::ReservationExpired {
                lease_expires_at: fence.lease_expires_at(),
            });
        }
        let outcome_window = prepared
            .outcome_timeout()
            .checked_add(self.options.outcome_grace)
            .ok_or_else(|| {
                ApplicationError::Internal("Connector execution outcome window overflowed".into())
            })?;
        if outcome_window.is_zero()
            || outcome_window
                > Duration::from_secs(MAXIMUM_CONNECTOR_EXECUTION_OUTCOME_SECONDS as u64)
        {
            return Err(ApplicationError::Internal(
                "Connector execution outcome window is invalid".into(),
            ));
        }
        let outcome_deadline_at = add_std(dispatch_started_at, outcome_window)?;
        let dispatch = self
            .attempts
            .begin_dispatch(
                BeginConnectorExecutionDispatch::new(
                    fence.clone(),
                    dispatch_started_at,
                    outcome_deadline_at,
                )
                .map_err(ApplicationError::Internal)?,
            )
            .await
            .map_err(map_attempt_repository_error)?;

        let (evidence, receipt) = match prepared.dispatch(&command.request).await {
            Ok(receipt) => {
                let evidence = ConnectorExecutionEvidence::accepted(
                    &revision,
                    &command.request,
                    &receipt,
                    dispatch_started_at,
                )
                .map_err(ApplicationError::Internal)?;
                (evidence, Some(receipt))
            }
            Err(error) => {
                let completed_at = canonical_timestamp(Utc::now()).max(dispatch_started_at);
                (
                    error_evidence(
                        &revision,
                        &command.request,
                        error,
                        dispatch_started_at,
                        completed_at,
                    )?,
                    None,
                )
            }
        };
        let settlement = SettleConnectorExecutionAttempt::new(fence, evidence.clone())
            .map_err(ApplicationError::Internal)?;
        match self.attempts.settle(settlement.clone()).await {
            Ok(write) => Ok(ConnectorExecutionAttemptResult::Completed {
                evidence: write.value.evidence.ok_or_else(|| {
                    ApplicationError::Internal(
                        "terminal Connector execution evidence is missing".into(),
                    )
                })?,
                receipt,
                replayed: write.replayed,
            }),
            Err(_) => {
                // No external call is ever retried here. The known fact can be
                // persisted separately; process death leaves the durable row
                // dispatching and therefore indeterminate after its deadline.
                debug_assert_eq!(
                    dispatch.attempt.dispatch_started_at(),
                    Some(dispatch_started_at)
                );
                Ok(ConnectorExecutionAttemptResult::SettlementPending {
                    settlement,
                    receipt,
                })
            }
        }
    }

    pub async fn settle_known(
        &self,
        settlement: SettleConnectorExecutionAttempt,
        resource_access: &ResourceAccessEvaluator,
    ) -> ApplicationResult<ConnectorExecutionAttemptResult> {
        settlement.validate().map_err(ApplicationError::Invalid)?;
        environment(
            settlement.fence.binding().project_id(),
            settlement.fence.binding().environment_id(),
            resource_access,
        )?;
        let write = self
            .attempts
            .settle(settlement)
            .await
            .map_err(map_attempt_repository_error)?;
        let evidence = write.value.evidence.ok_or_else(|| {
            ApplicationError::Internal("terminal Connector execution evidence is missing".into())
        })?;
        Ok(ConnectorExecutionAttemptResult::Completed {
            evidence,
            receipt: None,
            replayed: write.replayed,
        })
    }

    async fn settle_preflight(
        &self,
        fence: ConnectorExecutionFence,
        evidence: ConnectorExecutionEvidence,
    ) -> ApplicationResult<ConnectorExecutionAttemptResult> {
        let write = self
            .attempts
            .settle(
                SettleConnectorExecutionAttempt::new(fence, evidence)
                    .map_err(ApplicationError::Internal)?,
            )
            .await
            .map_err(map_attempt_repository_error)?;
        Ok(ConnectorExecutionAttemptResult::Completed {
            evidence: write.value.evidence.ok_or_else(|| {
                ApplicationError::Internal(
                    "terminal Connector preflight evidence is missing".into(),
                )
            })?,
            receipt: None,
            replayed: write.replayed,
        })
    }
}

fn validate_command(command: &ExecuteConnectorAttempt) -> ApplicationResult<()> {
    command
        .request
        .validate()
        .map_err(ApplicationError::Invalid)?;
    if command.organization_id.as_uuid().is_nil()
        || command.project_id.as_uuid().is_nil()
        || command.environment_id.as_uuid().is_nil()
        || command.profile_id.as_uuid().is_nil()
        || command.revision_id.as_uuid().is_nil()
        || command.fence_token.is_nil()
        || command.request.connector_revision_id() != command.revision_id
        || command.requested_at != canonical_timestamp(command.requested_at)
    {
        return Err(ApplicationError::Invalid(
            "Connector execution command is invalid".into(),
        ));
    }
    Ok(())
}

fn error_evidence(
    revision: &crate::modules::connectors::domain::ConnectorRevision,
    request: &ConnectorExecutionRequest,
    error: crate::modules::connectors::domain::ConnectorExecutionError,
    started_at: DateTime<Utc>,
    completed_at: DateTime<Utc>,
) -> ApplicationResult<ConnectorExecutionEvidence> {
    match error {
        crate::modules::connectors::domain::ConnectorExecutionError::Retryable { retry_after } => {
            ConnectorExecutionEvidence::retryable(
                revision,
                request,
                None,
                retry_after,
                started_at,
                completed_at,
            )
        }
        crate::modules::connectors::domain::ConnectorExecutionError::Rejected => {
            ConnectorExecutionEvidence::rejected(revision, request, None, started_at, completed_at)
        }
    }
    .map_err(ApplicationError::Internal)
}

fn completed_from_record(
    record: ConnectorExecutionAttemptRecord,
    replayed: bool,
) -> ApplicationResult<ConnectorExecutionAttemptResult> {
    Ok(ConnectorExecutionAttemptResult::Completed {
        evidence: record.evidence.ok_or_else(|| {
            ApplicationError::Internal("terminal Connector execution evidence is missing".into())
        })?,
        receipt: None,
        replayed,
    })
}

fn in_flight_result(
    record: &ConnectorExecutionAttemptRecord,
) -> ApplicationResult<ConnectorExecutionAttemptResult> {
    Ok(ConnectorExecutionAttemptResult::InFlight {
        dispatch_started_at: record.attempt.dispatch_started_at().ok_or_else(|| {
            ApplicationError::Internal("Connector dispatch start is missing".into())
        })?,
        outcome_deadline_at: record.attempt.outcome_deadline_at().ok_or_else(|| {
            ApplicationError::Internal("Connector dispatch deadline is missing".into())
        })?,
    })
}

fn indeterminate_result(
    record: &ConnectorExecutionAttemptRecord,
) -> ApplicationResult<ConnectorExecutionAttemptResult> {
    Ok(ConnectorExecutionAttemptResult::Indeterminate {
        dispatch_started_at: record.attempt.dispatch_started_at().ok_or_else(|| {
            ApplicationError::Internal("Connector dispatch start is missing".into())
        })?,
        outcome_deadline_at: record.attempt.outcome_deadline_at().ok_or_else(|| {
            ApplicationError::Internal("Connector dispatch deadline is missing".into())
        })?,
    })
}

fn add_std(value: DateTime<Utc>, duration: Duration) -> ApplicationResult<DateTime<Utc>> {
    let duration = ChronoDuration::from_std(duration).map_err(|_| {
        ApplicationError::Internal("Connector execution duration is invalid".into())
    })?;
    value
        .checked_add_signed(duration)
        .map(canonical_timestamp)
        .ok_or_else(|| {
            ApplicationError::Internal(
                "Connector execution time is outside the supported range".into(),
            )
        })
}

fn map_revision_repository_error(error: RepositoryError) -> ApplicationError {
    match error {
        RepositoryError::NotFound => revision_not_found(),
        other => other.into(),
    }
}

fn map_attempt_repository_error(error: RepositoryError) -> ApplicationError {
    match error {
        RepositoryError::NotFound => attempt_not_found(),
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::connectors::domain::{
        ConnectorDefinition, ConnectorExecutionAttemptCursor, ConnectorExecutionOutcome,
        ConnectorHttpAuthentication, ConnectorHttpDefinition, ConnectorHttpDefinitionSpec,
        ConnectorHttpDestination, ConnectorHttpMethod, ConnectorHttpStatusPolicy, ConnectorProfile,
        ConnectorRecord, ConnectorRevision, ConnectorRevisionPublished,
        CreateConnectorProfileWrite, IPreparedConnectorExecution,
    };
    use crate::modules::connectors::infrastructure::{
        InMemoryConnectorExecutionRepository, InMemoryConnectorProfileRepository,
    };
    use crate::modules::identity::domain::value_objects::ResourceGrantScope;
    use crate::modules::shared_kernel::domain::{
        IdempotencyRequest, IdempotentWrite, PrincipalId, ResourceName,
    };
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Clone, Copy)]
    enum PreparedOutcome {
        Accepted,
        RetryablePreparation,
    }

    struct RecordingPreparation {
        prepares: Arc<AtomicUsize>,
        dispatches: Arc<AtomicUsize>,
        outcome: PreparedOutcome,
    }

    #[async_trait]
    impl IConnectorExecutionPreparationPort for RecordingPreparation {
        async fn prepare(
            &self,
            _revision: &ConnectorRevision,
            request: &ConnectorExecutionRequest,
        ) -> Result<
            Box<dyn IPreparedConnectorExecution>,
            crate::modules::connectors::domain::ConnectorExecutionError,
        > {
            self.prepares.fetch_add(1, Ordering::SeqCst);
            match self.outcome {
                PreparedOutcome::RetryablePreparation => Err(
                    crate::modules::connectors::domain::ConnectorExecutionError::Retryable {
                        retry_after: None,
                    },
                ),
                PreparedOutcome::Accepted => Ok(Box::new(RecordingPrepared {
                    revision_id: request.connector_revision_id(),
                    attempt_id: request.attempt_id(),
                    dispatches: self.dispatches.clone(),
                })),
            }
        }
    }

    struct RecordingPrepared {
        revision_id: ConnectorRevisionId,
        attempt_id: Uuid,
        dispatches: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl IPreparedConnectorExecution for RecordingPrepared {
        fn outcome_timeout(&self) -> Duration {
            Duration::from_secs(1)
        }

        async fn dispatch(
            self: Box<Self>,
            _request: &ConnectorExecutionRequest,
        ) -> Result<
            ConnectorExecutionReceipt,
            crate::modules::connectors::domain::ConnectorExecutionError,
        > {
            self.dispatches.fetch_add(1, Ordering::SeqCst);
            ConnectorExecutionReceipt::accepted(
                self.revision_id,
                self.attempt_id,
                canonical_timestamp(Utc::now()),
                202,
                None,
                b"bounded-response".to_vec(),
            )
        }
    }

    struct FailFirstSettlement {
        inner: Arc<InMemoryConnectorExecutionRepository>,
        fail: AtomicBool,
    }

    #[async_trait]
    impl IConnectorExecutionAttemptRepository for FailFirstSettlement {
        async fn reserve(
            &self,
            request: ReserveConnectorExecutionAttempt,
        ) -> Result<ConnectorExecutionReservation, RepositoryError> {
            self.inner.reserve(request).await
        }

        async fn begin_dispatch(
            &self,
            request: BeginConnectorExecutionDispatch,
        ) -> Result<ConnectorExecutionAttemptRecord, RepositoryError> {
            self.inner.begin_dispatch(request).await
        }

        async fn settle(
            &self,
            request: SettleConnectorExecutionAttempt,
        ) -> Result<IdempotentWrite<ConnectorExecutionAttemptRecord>, RepositoryError> {
            if self.fail.swap(false, Ordering::SeqCst) {
                return Err(RepositoryError::Storage(
                    "injected settlement uncertainty".into(),
                ));
            }
            self.inner.settle(request).await
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
            self.inner
                .find(
                    organization_id,
                    project_id,
                    environment_id,
                    profile_id,
                    revision_id,
                    attempt_id,
                )
                .await
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
            self.inner
                .list_unresolved_page(
                    organization_id,
                    project_id,
                    environment_id,
                    profile_id,
                    revision_id,
                    after,
                    limit,
                )
                .await
        }
    }

    struct Fixture {
        profiles: Arc<InMemoryConnectorProfileRepository>,
        attempts: Arc<InMemoryConnectorExecutionRepository>,
        revision: ConnectorRevision,
        prepares: Arc<AtomicUsize>,
        dispatches: Arc<AtomicUsize>,
    }

    async fn fixture() -> Fixture {
        let now = canonical_timestamp(Utc::now());
        let revision = ConnectorRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
            ConnectorDefinition::Http(
                ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
                    destination: ConnectorHttpDestination::LiteralHttps {
                        endpoint: "https://hooks.example.test/fenced-service".into(),
                    },
                    method: ConnectorHttpMethod::Post,
                    request_content_type: "application/json".into(),
                    maximum_request_bytes: 1024,
                    maximum_response_bytes: 1024,
                    timeout_milliseconds: 1_000,
                    status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
                    authentication: ConnectorHttpAuthentication::None,
                })
                .expect("definition"),
            ),
            PrincipalId::new(),
            now,
        )
        .expect("revision");
        let profile = ConnectorProfile::create(
            revision.profile_id,
            ResourceName::parse("Fenced Connector").expect("name"),
            &revision,
        )
        .expect("profile");
        let record = ConnectorRecord::new(profile, revision.clone()).expect("record");
        let request_id = Uuid::now_v7();
        let profiles = Arc::new(InMemoryConnectorProfileRepository::new());
        profiles
            .create(CreateConnectorProfileWrite {
                event: ConnectorRevisionPublished::created(
                    &record.profile,
                    &record.revision,
                    request_id,
                )
                .expect("event"),
                actor_principal_id: revision.created_by,
                request_id,
                idempotency: IdempotencyRequest::new(
                    "connector-execution-service-test",
                    "profile",
                    revision.definition.digest().as_str().as_bytes(),
                )
                .expect("idempotency"),
                record,
            })
            .await
            .expect("store profile");
        Fixture {
            profiles,
            attempts: Arc::new(InMemoryConnectorExecutionRepository::new()),
            revision,
            prepares: Arc::new(AtomicUsize::new(0)),
            dispatches: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn command(
        revision: &ConnectorRevision,
        requested_at: DateTime<Utc>,
    ) -> ExecuteConnectorAttempt {
        ExecuteConnectorAttempt {
            organization_id: revision.organization_id,
            project_id: revision.project_id,
            environment_id: revision.environment_id,
            profile_id: revision.profile_id,
            revision_id: revision.id,
            request: ConnectorExecutionRequest::new(
                revision.id,
                Uuid::now_v7(),
                "application/json",
                b"request".to_vec(),
            )
            .expect("request"),
            resource_access: ResourceAccessEvaluator::organization_wide(),
            fence_token: Uuid::now_v7(),
            requested_at,
        }
    }

    fn preparation(fixture: &Fixture, outcome: PreparedOutcome) -> Arc<RecordingPreparation> {
        Arc::new(RecordingPreparation {
            prepares: fixture.prepares.clone(),
            dispatches: fixture.dispatches.clone(),
            outcome,
        })
    }

    #[tokio::test]
    async fn terminal_replay_never_prepares_or_dispatches_twice() {
        let fixture = fixture().await;
        let service = ConnectorExecutionApplicationService::new(
            fixture.profiles.clone(),
            fixture.attempts.clone(),
            preparation(&fixture, PreparedOutcome::Accepted),
            ConnectorExecutionServiceOptions::default(),
        )
        .expect("service");
        let command = command(&fixture.revision, canonical_timestamp(Utc::now()));
        let first = service.execute(command.clone()).await.expect("execute");
        assert!(matches!(
            first,
            ConnectorExecutionAttemptResult::Completed {
                receipt: Some(_),
                replayed: false,
                ..
            }
        ));
        let replay = service.execute(command).await.expect("replay");
        assert!(matches!(
            replay,
            ConnectorExecutionAttemptResult::Completed {
                receipt: None,
                replayed: true,
                ..
            }
        ));
        assert_eq!(fixture.prepares.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.dispatches.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn preparation_failure_settles_without_crossing_dispatch_boundary() {
        let fixture = fixture().await;
        let service = ConnectorExecutionApplicationService::new(
            fixture.profiles.clone(),
            fixture.attempts.clone(),
            preparation(&fixture, PreparedOutcome::RetryablePreparation),
            ConnectorExecutionServiceOptions::default(),
        )
        .expect("service");
        let result = service
            .execute(command(&fixture.revision, canonical_timestamp(Utc::now())))
            .await
            .expect("execute");
        assert!(matches!(
            result,
            ConnectorExecutionAttemptResult::Completed {
                evidence,
                receipt: None,
                ..
            } if evidence.outcome() == ConnectorExecutionOutcome::Retryable
        ));
        assert_eq!(fixture.prepares.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.dispatches.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn failed_evidence_commit_returns_settlement_only_and_never_blind_retries() {
        let fixture = fixture().await;
        let failing = Arc::new(FailFirstSettlement {
            inner: fixture.attempts.clone(),
            fail: AtomicBool::new(true),
        });
        let service = ConnectorExecutionApplicationService::new(
            fixture.profiles.clone(),
            failing,
            preparation(&fixture, PreparedOutcome::Accepted),
            ConnectorExecutionServiceOptions::default(),
        )
        .expect("service");
        let command = command(&fixture.revision, canonical_timestamp(Utc::now()));
        let pending = match service.execute(command.clone()).await.expect("execute") {
            ConnectorExecutionAttemptResult::SettlementPending { settlement, .. } => settlement,
            other => panic!("unexpected result: {other:?}"),
        };
        assert_eq!(fixture.dispatches.load(Ordering::SeqCst), 1);
        assert!(matches!(
            service.execute(command).await.expect("recover observation"),
            ConnectorExecutionAttemptResult::InFlight { .. }
                | ConnectorExecutionAttemptResult::Indeterminate { .. }
        ));
        assert_eq!(fixture.dispatches.load(Ordering::SeqCst), 1);
        assert!(matches!(
            service
                .settle_known(pending, &ResourceAccessEvaluator::organization_wide())
                .await
                .expect("settle known"),
            ConnectorExecutionAttemptResult::Completed { .. }
        ));
        assert_eq!(fixture.dispatches.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn expired_dispatch_is_indeterminate_without_preparation_or_provider_call() {
        let fixture = fixture().await;
        let observed_at = canonical_timestamp(Utc::now());
        let mut command = command(&fixture.revision, observed_at - ChronoDuration::seconds(20));
        let binding =
            ConnectorExecutionAttemptBinding::from_exact(&fixture.revision, &command.request)
                .expect("binding");
        let fence = match fixture
            .attempts
            .reserve(
                ReserveConnectorExecutionAttempt::new(
                    binding,
                    command.fence_token,
                    command.requested_at,
                    command.requested_at + ChronoDuration::seconds(30),
                )
                .expect("reservation"),
            )
            .await
            .expect("reserve")
        {
            ConnectorExecutionReservation::Acquired { fence, .. } => fence,
            other => panic!("unexpected reservation: {other:?}"),
        };
        let started_at = command.requested_at + ChronoDuration::seconds(1);
        fixture
            .attempts
            .begin_dispatch(
                BeginConnectorExecutionDispatch::new(
                    fence,
                    started_at,
                    started_at + ChronoDuration::seconds(5),
                )
                .expect("dispatch"),
            )
            .await
            .expect("begin dispatch");
        command.requested_at = observed_at;
        command.fence_token = Uuid::now_v7();
        let service = ConnectorExecutionApplicationService::new(
            fixture.profiles.clone(),
            fixture.attempts.clone(),
            preparation(&fixture, PreparedOutcome::Accepted),
            ConnectorExecutionServiceOptions::default(),
        )
        .expect("service");
        assert!(matches!(
            service.execute(command).await.expect("recover"),
            ConnectorExecutionAttemptResult::Indeterminate { .. }
        ));
        assert_eq!(fixture.prepares.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.dispatches.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn authorization_denial_happens_before_revision_or_attempt_work() {
        let fixture = fixture().await;
        let service = ConnectorExecutionApplicationService::new(
            fixture.profiles.clone(),
            fixture.attempts.clone(),
            preparation(&fixture, PreparedOutcome::Accepted),
            ConnectorExecutionServiceOptions::default(),
        )
        .expect("service");
        let mut command = command(&fixture.revision, canonical_timestamp(Utc::now()));
        command.resource_access =
            ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
                project_id: fixture.revision.project_id,
                environment_id: EnvironmentId::new(),
            }]);
        assert!(matches!(
            service.execute(command).await,
            Err(ApplicationError::NotFound(_))
        ));
        assert_eq!(fixture.prepares.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.dispatches.load(Ordering::SeqCst), 0);
    }
}
