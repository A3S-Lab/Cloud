use super::{
    ConnectorExecutionApplicationService, ConnectorExecutionAttemptResult, ExecuteConnectorAttempt,
};
use crate::modules::connectors::domain::{
    ConnectorExecutionEvidence, ConnectorExecutionOutcome, ConnectorExecutionRequest,
    ConnectorResponseObjectReference, MAXIMUM_CONNECTOR_BODY_BYTES,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, ConnectorProfileId, ConnectorRevisionId,
    EnvironmentId, OrganizationId, PlanRevisionId, ProjectId, Sha256Digest, WorkflowRunId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

pub const WORKFLOW_CONNECTOR_CAPABILITY: &str = "connector.http";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowConnectorResponseMode {
    DigestOnly,
    ImmutableObjectReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum WorkflowConnectorAttemptPurpose {
    #[default]
    Normal,
    CancellationCompensation {
        source_step_id: String,
    },
}

/// One Flow-owned attempt against an exact, immutable Connector revision.
///
/// The request carries Workflow authority but no endpoint, credential, retry
/// schedule, or provider-specific fields. The effective input becomes bounded
/// canonical JSON, while C6 verifies the exact revision before reservation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowConnectorAttemptRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub step_id: String,
    pub step_attempt: u32,
    pub purpose: WorkflowConnectorAttemptPurpose,
    pub connector_profile_id: ConnectorProfileId,
    pub connector_revision_id: ConnectorRevisionId,
    pub connector_revision_digest: Sha256Digest,
    pub capability: String,
    pub input: serde_json::Value,
    pub response_mode: WorkflowConnectorResponseMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowConnectorAttemptAuthority {
    pub attempt_id: Uuid,
    pub request_digest: Sha256Digest,
    pub request_body_bytes: u64,
}

impl WorkflowConnectorAttemptRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.connector_profile_id.as_uuid().is_nil()
            || self.connector_revision_id.as_uuid().is_nil()
            || self.step_attempt == 0
            || self.capability != WORKFLOW_CONNECTOR_CAPABILITY
            || self.step_id.is_empty()
            || self.step_id.len() > 96
            || !self
                .step_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err("Workflow Connector attempt authority is invalid".into());
        }
        if let WorkflowConnectorAttemptPurpose::CancellationCompensation { source_step_id } =
            &self.purpose
        {
            if source_step_id == &self.step_id
                || source_step_id.is_empty()
                || source_step_id.len() > 96
                || !source_step_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            {
                return Err("Workflow Connector attempt purpose is invalid".into());
            }
        }
        Sha256Digest::parse(self.plan_digest.as_str())?;
        Sha256Digest::parse(self.connector_revision_digest.as_str())?;
        canonical_json_bounded(
            &self.input,
            MAXIMUM_CONNECTOR_BODY_BYTES,
            "Workflow Connector effective input",
        )?;
        Ok(())
    }

    /// Derives the C6 attempt identity from immutable Workflow and Connector
    /// authority. A Flow redelivery reuses it; a Flow retry generation does not.
    pub fn connector_attempt_id(&self) -> Result<Uuid, String> {
        self.validate()?;
        let identity = match &self.purpose {
            WorkflowConnectorAttemptPurpose::Normal => format!(
                "a3s.cloud.workflow-connector-attempt.v1\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
                self.plan_revision_id,
                self.plan_digest,
                self.step_id,
                self.step_attempt,
                self.connector_profile_id,
                self.connector_revision_id,
                self.connector_revision_digest,
            ),
            WorkflowConnectorAttemptPurpose::CancellationCompensation { source_step_id } => {
                format!(
                    "a3s.cloud.workflow-connector-cancellation-compensation-attempt.v1\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
                    self.plan_revision_id,
                    self.plan_digest,
                    source_step_id,
                    self.step_id,
                    self.step_attempt,
                    self.connector_profile_id,
                    self.connector_revision_id,
                    self.connector_revision_digest,
                )
            }
        };
        Ok(Uuid::new_v5(
            &self.workflow_run_id.as_uuid(),
            identity.as_bytes(),
        ))
    }

    pub fn connector_attempt_authority(&self) -> Result<WorkflowConnectorAttemptAuthority, String> {
        let request = self.connector_request()?;
        Ok(WorkflowConnectorAttemptAuthority {
            attempt_id: request.attempt_id(),
            request_digest: request.evidence_digest(),
            request_body_bytes: request.body().len() as u64,
        })
    }

    fn connector_request(&self) -> Result<ConnectorExecutionRequest, String> {
        self.validate()?;
        let body = canonical_json_bounded(
            &self.input,
            MAXIMUM_CONNECTOR_BODY_BYTES,
            "Workflow Connector effective input",
        )?;
        ConnectorExecutionRequest::new(
            self.connector_revision_id,
            self.connector_attempt_id()?,
            "application/json",
            body,
        )
    }
}

/// Recoverable result exposed to Workflow without leaking Connector fencing or
/// transient provider response bodies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowConnectorAttemptResult {
    Completed {
        evidence: Box<ConnectorExecutionEvidence>,
        response_object: Option<ConnectorResponseObjectReference>,
    },
    Deferred {
        attempt_id: Uuid,
        retry_not_before: DateTime<Utc>,
    },
    Indeterminate {
        attempt_id: Uuid,
        dispatch_started_at: DateTime<Utc>,
        outcome_deadline_at: DateTime<Utc>,
    },
}

#[async_trait]
pub trait IWorkflowConnectorPort: Send + Sync {
    /// Executes or observes exactly one Flow-owned attempt. The caller remains
    /// the sole retry, backoff, cancellation, and terminal-failure authority.
    async fn execute_attempt(
        &self,
        request: &WorkflowConnectorAttemptRequest,
    ) -> ApplicationResult<WorkflowConnectorAttemptResult>;
}

#[derive(Clone)]
pub struct WorkflowConnectorApplicationService {
    executions: Arc<ConnectorExecutionApplicationService>,
}

impl WorkflowConnectorApplicationService {
    pub fn new(executions: Arc<ConnectorExecutionApplicationService>) -> Self {
        Self { executions }
    }

    fn validate_evidence(
        request: &WorkflowConnectorAttemptRequest,
        connector_request: &ConnectorExecutionRequest,
        evidence: &ConnectorExecutionEvidence,
    ) -> ApplicationResult<()> {
        evidence.validate().map_err(ApplicationError::Internal)?;
        if evidence.organization_id() != request.organization_id
            || evidence.project_id() != request.project_id
            || evidence.environment_id() != request.environment_id
            || evidence.profile_id() != request.connector_profile_id
            || evidence.revision_id() != request.connector_revision_id
            || evidence.attempt_id() != connector_request.attempt_id()
            || evidence.request_digest() != &connector_request.evidence_digest()
            || evidence.request_body_bytes() != connector_request.body().len() as u64
        {
            return Err(ApplicationError::Conflict(
                "Workflow Connector evidence changed its exact attempt authority".into(),
            ));
        }
        Ok(())
    }

    fn validate_response_object(
        request: &WorkflowConnectorAttemptRequest,
        evidence: &ConnectorExecutionEvidence,
        response_object: Option<&ConnectorResponseObjectReference>,
    ) -> ApplicationResult<()> {
        match (request.response_mode, evidence.outcome(), response_object) {
            (WorkflowConnectorResponseMode::DigestOnly, _, None) => Ok(()),
            (
                WorkflowConnectorResponseMode::ImmutableObjectReference,
                ConnectorExecutionOutcome::Accepted,
                Some(reference),
            ) => reference
                .validate_evidence(evidence)
                .map_err(ApplicationError::Internal),
            (
                WorkflowConnectorResponseMode::ImmutableObjectReference,
                ConnectorExecutionOutcome::Retryable
                | ConnectorExecutionOutcome::Rejected
                | ConnectorExecutionOutcome::Indeterminate,
                None,
            ) => Ok(()),
            _ => Err(ApplicationError::Internal(
                "Workflow Connector response-object result is inconsistent".into(),
            )),
        }
    }

    async fn normalize_result(
        &self,
        request: &WorkflowConnectorAttemptRequest,
        connector_request: &ConnectorExecutionRequest,
        resource_access: &ResourceAccessEvaluator,
        result: ConnectorExecutionAttemptResult,
    ) -> ApplicationResult<WorkflowConnectorAttemptResult> {
        let result = match result {
            ConnectorExecutionAttemptResult::SettlementPending {
                settlement,
                response_object,
                ..
            } => {
                let settled = self
                    .executions
                    .settle_known(settlement, resource_access)
                    .await?;
                match settled {
                    ConnectorExecutionAttemptResult::Completed {
                        evidence,
                        receipt,
                        replayed,
                        ..
                    } => ConnectorExecutionAttemptResult::Completed {
                        evidence,
                        response_object,
                        receipt,
                        replayed,
                    },
                    other => other,
                }
            }
            result => result,
        };
        match result {
            ConnectorExecutionAttemptResult::Completed {
                evidence,
                response_object,
                ..
            } => {
                Self::validate_evidence(request, connector_request, &evidence)?;
                Self::validate_response_object(request, &evidence, response_object.as_ref())?;
                Ok(WorkflowConnectorAttemptResult::Completed {
                    evidence: Box::new(evidence),
                    response_object,
                })
            }
            ConnectorExecutionAttemptResult::Reserved { lease_expires_at }
            | ConnectorExecutionAttemptResult::ReservationExpired { lease_expires_at } => {
                Ok(WorkflowConnectorAttemptResult::Deferred {
                    attempt_id: connector_request.attempt_id(),
                    retry_not_before: lease_expires_at,
                })
            }
            ConnectorExecutionAttemptResult::InFlight {
                outcome_deadline_at,
                ..
            } => Ok(WorkflowConnectorAttemptResult::Deferred {
                attempt_id: connector_request.attempt_id(),
                retry_not_before: outcome_deadline_at,
            }),
            ConnectorExecutionAttemptResult::Indeterminate {
                dispatch_started_at,
                outcome_deadline_at,
            } => Ok(WorkflowConnectorAttemptResult::Indeterminate {
                attempt_id: connector_request.attempt_id(),
                dispatch_started_at,
                outcome_deadline_at,
            }),
            ConnectorExecutionAttemptResult::SettlementPending { .. } => {
                Err(ApplicationError::Internal(
                    "known Workflow Connector settlement did not become terminal".into(),
                ))
            }
        }
    }
}

#[async_trait]
impl IWorkflowConnectorPort for WorkflowConnectorApplicationService {
    async fn execute_attempt(
        &self,
        request: &WorkflowConnectorAttemptRequest,
    ) -> ApplicationResult<WorkflowConnectorAttemptResult> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let connector_request = request
            .connector_request()
            .map_err(ApplicationError::Invalid)?;
        let resource_access =
            ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
                project_id: request.project_id,
                environment_id: request.environment_id,
            }]);
        let command = ExecuteConnectorAttempt {
            organization_id: request.organization_id,
            project_id: request.project_id,
            environment_id: request.environment_id,
            profile_id: request.connector_profile_id,
            revision_id: request.connector_revision_id,
            request: connector_request.clone(),
            resource_access: resource_access.clone(),
            fence_token: Uuid::now_v7(),
            requested_at: canonical_timestamp(Utc::now()),
        };
        let result = match request.response_mode {
            WorkflowConnectorResponseMode::DigestOnly => {
                self.executions
                    .execute_exact(command, &request.connector_revision_digest)
                    .await?
            }
            WorkflowConnectorResponseMode::ImmutableObjectReference => {
                self.executions
                    .execute_exact_with_response_object(command, &request.connector_revision_digest)
                    .await?
            }
        };
        self.normalize_result(request, &connector_request, &resource_access, result)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::ImmutableObjectClient;
    use crate::modules::connectors::domain::{
        ConnectorDefinition, ConnectorExecutionError, ConnectorExecutionReceipt,
        ConnectorHttpAuthentication, ConnectorHttpDefinition, ConnectorHttpDefinitionSpec,
        ConnectorHttpDestination, ConnectorHttpMethod, ConnectorHttpStatusPolicy, ConnectorProfile,
        ConnectorRecord, ConnectorRevision, ConnectorRevisionPublished,
        CreateConnectorProfileWrite, IConnectorExecutionPreparationPort,
        IConnectorProfileRepository, IPreparedConnectorExecution,
    };
    use crate::modules::connectors::infrastructure::{
        ConnectorResponseObjectStore, InMemoryConnectorExecutionRepository,
        InMemoryConnectorProfileRepository,
    };
    use crate::modules::shared_kernel::domain::{IdempotencyRequest, PrincipalId, ResourceName};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    struct AcceptedPreparation {
        dispatches: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl IConnectorExecutionPreparationPort for AcceptedPreparation {
        async fn prepare(
            &self,
            _revision: &ConnectorRevision,
            request: &ConnectorExecutionRequest,
        ) -> Result<Box<dyn IPreparedConnectorExecution>, ConnectorExecutionError> {
            Ok(Box::new(AcceptedPrepared {
                revision_id: request.connector_revision_id(),
                attempt_id: request.attempt_id(),
                dispatches: Arc::clone(&self.dispatches),
            }))
        }
    }

    struct AcceptedPrepared {
        revision_id: ConnectorRevisionId,
        attempt_id: Uuid,
        dispatches: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl IPreparedConnectorExecution for AcceptedPrepared {
        fn outcome_timeout(&self) -> Duration {
            Duration::from_secs(1)
        }

        async fn dispatch(
            self: Box<Self>,
            _request: &ConnectorExecutionRequest,
        ) -> Result<ConnectorExecutionReceipt, ConnectorExecutionError> {
            self.dispatches.fetch_add(1, Ordering::SeqCst);
            ConnectorExecutionReceipt::accepted(
                self.revision_id,
                self.attempt_id,
                canonical_timestamp(Utc::now()),
                200,
                Some("application/json".into()),
                br#"{"accepted":true}"#.to_vec(),
            )
        }
    }

    struct Fixture {
        port: WorkflowConnectorApplicationService,
        revision: ConnectorRevision,
        dispatches: Arc<AtomicUsize>,
    }

    async fn fixture() -> Fixture {
        let revision = ConnectorRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
            ConnectorDefinition::Http(
                ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
                    destination: ConnectorHttpDestination::LiteralHttps {
                        endpoint: "https://workflow.example.test/invoke".into(),
                    },
                    method: ConnectorHttpMethod::Post,
                    request_content_type: "application/json".into(),
                    maximum_request_bytes: 64 * 1024,
                    maximum_response_bytes: 64 * 1024,
                    timeout_milliseconds: 1_000,
                    status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
                    authentication: ConnectorHttpAuthentication::None,
                })
                .expect("definition"),
            ),
            PrincipalId::new(),
            canonical_timestamp(Utc::now()),
        )
        .expect("revision");
        let profile = ConnectorProfile::create(
            revision.profile_id,
            ResourceName::parse("Workflow Connector").expect("profile name"),
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
                    "workflow-connector-port-test",
                    "profile",
                    revision.definition.digest().as_str().as_bytes(),
                )
                .expect("idempotency"),
                record,
            })
            .await
            .expect("store profile");
        let dispatches = Arc::new(AtomicUsize::new(0));
        let response_objects: Arc<dyn object_store::ObjectStore> =
            Arc::new(object_store::memory::InMemory::new());
        let response_objects = Arc::new(ConnectorResponseObjectStore::from_client(
            ImmutableObjectClient::from_store(response_objects, "connector-responses")
                .expect("response object client"),
        ));
        let executions = Arc::new(
            ConnectorExecutionApplicationService::new(
                profiles.clone(),
                Arc::new(InMemoryConnectorExecutionRepository::new()),
                Arc::new(AcceptedPreparation {
                    dispatches: Arc::clone(&dispatches),
                }),
                super::super::ConnectorExecutionServiceOptions::default(),
            )
            .expect("execution service")
            .with_response_object_store(response_objects),
        );
        Fixture {
            port: WorkflowConnectorApplicationService::new(executions),
            revision,
            dispatches,
        }
    }

    fn request(revision: &ConnectorRevision) -> WorkflowConnectorAttemptRequest {
        WorkflowConnectorAttemptRequest {
            organization_id: revision.organization_id,
            project_id: revision.project_id,
            environment_id: revision.environment_id,
            workflow_run_id: WorkflowRunId::new(),
            plan_revision_id: PlanRevisionId::new(),
            plan_digest: digest('a'),
            step_id: "send-request".into(),
            step_attempt: 1,
            purpose: WorkflowConnectorAttemptPurpose::Normal,
            connector_profile_id: revision.profile_id,
            connector_revision_id: revision.id,
            connector_revision_digest: revision.definition.digest().clone(),
            capability: WORKFLOW_CONNECTOR_CAPABILITY.into(),
            input: serde_json::json!({"ticketId": "T-42", "priority": "high"}),
            response_mode: WorkflowConnectorResponseMode::DigestOnly,
        }
    }

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    #[tokio::test]
    async fn exact_flow_attempt_replays_one_c6_evidence_without_a_second_dispatch() {
        let fixture = fixture().await;
        let request = request(&fixture.revision);
        let expected_attempt_id = request.connector_attempt_id().expect("attempt ID");

        let first = fixture
            .port
            .execute_attempt(&request)
            .await
            .expect("execute attempt");
        let evidence = match &first {
            WorkflowConnectorAttemptResult::Completed {
                evidence,
                response_object: None,
            } => evidence,
            other => panic!("unexpected result: {other:?}"),
        };
        assert_eq!(evidence.attempt_id(), expected_attempt_id);
        assert_eq!(evidence.response_status(), Some(200));
        assert_eq!(
            evidence.response_digest(),
            Some(&Sha256Digest::from_bytes(br#"{"accepted":true}"#))
        );
        assert_eq!(
            evidence.response_body_bytes(),
            Some(br#"{"accepted":true}"#.len() as u64)
        );

        let replay = fixture
            .port
            .execute_attempt(&request)
            .await
            .expect("replay attempt");
        assert_eq!(replay, first);
        assert_eq!(fixture.dispatches.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn immutable_response_mode_returns_the_same_verified_object_on_replay() {
        let fixture = fixture().await;
        let mut request = request(&fixture.revision);
        request.response_mode = WorkflowConnectorResponseMode::ImmutableObjectReference;

        let first = fixture
            .port
            .execute_attempt(&request)
            .await
            .expect("execute response-object attempt");
        let reference = match &first {
            WorkflowConnectorAttemptResult::Completed {
                evidence,
                response_object: Some(reference),
            } => {
                reference
                    .validate_evidence(evidence)
                    .expect("response object authority");
                reference.clone()
            }
            other => panic!("unexpected result: {other:?}"),
        };

        let replay = fixture
            .port
            .execute_attempt(&request)
            .await
            .expect("replay response-object attempt");
        assert_eq!(replay, first);
        assert!(matches!(
            replay,
            WorkflowConnectorAttemptResult::Completed {
                response_object: Some(replayed),
                ..
            } if replayed == reference
        ));
        assert_eq!(fixture.dispatches.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn revision_digest_drift_fails_before_c6_dispatch() {
        let fixture = fixture().await;
        let mut request = request(&fixture.revision);
        request.connector_revision_digest = digest('f');

        assert!(matches!(
            fixture.port.execute_attempt(&request).await,
            Err(ApplicationError::Conflict(_))
        ));
        assert_eq!(fixture.dispatches.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn attempt_identity_is_stable_for_redelivery_and_changes_with_flow_authority() {
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
                        endpoint: "https://workflow.example.test/identity".into(),
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
        let request = request(&revision);
        let first = request.connector_attempt_id().expect("attempt ID");
        assert_eq!(
            request.connector_attempt_id().expect("redelivery ID"),
            first
        );

        let mut next_attempt = request.clone();
        next_attempt.step_attempt = 2;
        assert_ne!(
            next_attempt.connector_attempt_id().expect("next attempt"),
            first
        );

        let mut compensation = request.clone();
        compensation.purpose = WorkflowConnectorAttemptPurpose::CancellationCompensation {
            source_step_id: "reserve".into(),
        };
        let compensation_id = compensation
            .connector_attempt_id()
            .expect("compensation attempt");
        assert_ne!(compensation_id, first);
        assert_eq!(
            compensation
                .connector_attempt_id()
                .expect("compensation redelivery"),
            compensation_id
        );

        let mut drifted_plan = request;
        drifted_plan.plan_digest = digest('b');
        assert_ne!(
            drifted_plan.connector_attempt_id().expect("drifted plan"),
            first
        );
    }
}
