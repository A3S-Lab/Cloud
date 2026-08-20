use super::{
    ConnectorExecutionApplicationService, IConnectorResponseObjectPort, ReadConnectorResponseObject,
};
use crate::modules::connectors::domain::{
    BeginConnectorExecutionDispatch, ConnectorDefinition, ConnectorExecutionAttemptBinding,
    ConnectorExecutionEvidence, ConnectorExecutionReceipt, ConnectorExecutionRequest,
    ConnectorExecutionReservation, ConnectorHttpAuthentication, ConnectorHttpDefinition,
    ConnectorHttpDefinitionSpec, ConnectorHttpDestination, ConnectorHttpMethod,
    ConnectorHttpStatusPolicy, ConnectorResponseObjectError, ConnectorResponseObjectReference,
    ConnectorResponseObjectWrite, ConnectorRevision, IConnectorExecutionAttemptRepository,
    IConnectorResponseObjectStore, ReserveConnectorExecutionAttempt,
    SettleConnectorExecutionAttempt,
};
use crate::modules::connectors::infrastructure::{
    ConnectorResponseObjectStore, InMemoryConnectorExecutionRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId,
    PrincipalId, ProjectId, Sha256Digest,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::Arc;
use tempfile::TempDir;
use uuid::Uuid;

struct ResponseFixture {
    _directory: TempDir,
    attempts: Arc<InMemoryConnectorExecutionRepository>,
    objects: Arc<ConnectorResponseObjectStore>,
    reference: ConnectorResponseObjectReference,
    body: Vec<u8>,
}

struct TestResponseObjectPort {
    attempts: Arc<dyn IConnectorExecutionAttemptRepository>,
    objects: Option<Arc<dyn IConnectorResponseObjectStore>>,
}

#[async_trait]
impl IConnectorResponseObjectPort for TestResponseObjectPort {
    async fn read_response_object(
        &self,
        request: &ReadConnectorResponseObject,
    ) -> Result<super::ConnectorResponseObjectContent, ApplicationError> {
        super::response_object_reader::read_response_object(
            self.attempts.as_ref(),
            self.objects.as_deref(),
            request,
        )
        .await
    }
}

impl ResponseFixture {
    fn service(&self) -> TestResponseObjectPort {
        TestResponseObjectPort {
            attempts: self.attempts.clone(),
            objects: Some(self.objects.clone()),
        }
    }

    fn read(&self) -> ReadConnectorResponseObject {
        ReadConnectorResponseObject {
            reference: self.reference.clone(),
            resource_access: ResourceAccessEvaluator::restricted([
                ResourceGrantScope::Environment {
                    project_id: self.reference.project_id,
                    environment_id: self.reference.environment_id,
                },
            ]),
        }
    }
}

async fn fixture(store_body: bool, settle_attempt: bool) -> ResponseFixture {
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
                    endpoint: "https://response.example.test/consume".into(),
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
    let request = ConnectorExecutionRequest::new(
        revision.id,
        Uuid::now_v7(),
        "application/json",
        br#"{"request":true}"#.to_vec(),
    )
    .expect("request");
    let attempts = Arc::new(InMemoryConnectorExecutionRepository::new());
    let reservation = ReserveConnectorExecutionAttempt::new(
        ConnectorExecutionAttemptBinding::from_exact(&revision, &request).expect("binding"),
        Uuid::now_v7(),
        now,
        now + Duration::seconds(20),
    )
    .expect("reservation");
    let fence = match attempts.reserve(reservation).await.expect("reserve") {
        ConnectorExecutionReservation::Acquired { fence, .. } => fence,
        other => panic!("unexpected reservation: {other:?}"),
    };
    let started_at = now + Duration::seconds(1);
    attempts
        .begin_dispatch(
            BeginConnectorExecutionDispatch::new(
                fence.clone(),
                started_at,
                started_at + Duration::seconds(10),
            )
            .expect("dispatch"),
        )
        .await
        .expect("begin dispatch");
    let body = br#"{"accepted":true,"source":"connector"}"#.to_vec();
    let receipt = ConnectorExecutionReceipt::accepted(
        revision.id,
        request.attempt_id(),
        started_at + Duration::seconds(1),
        200,
        Some("application/json".into()),
        body.clone(),
    )
    .expect("receipt");
    let evidence = ConnectorExecutionEvidence::accepted(&revision, &request, &receipt, started_at)
        .expect("evidence");
    let reference =
        ConnectorResponseObjectReference::from_accepted(&revision, &receipt).expect("reference");
    let directory = tempfile::tempdir().expect("response object directory");
    let objects = Arc::new(ConnectorResponseObjectStore::local(directory.path()).expect("store"));
    if store_body {
        objects
            .put(&reference, body.clone())
            .await
            .expect("store response");
    }
    if settle_attempt {
        attempts
            .settle(SettleConnectorExecutionAttempt::new(fence, evidence).expect("settlement"))
            .await
            .expect("settle");
    }
    ResponseFixture {
        _directory: directory,
        attempts,
        objects,
        reference,
        body,
    }
}

#[tokio::test]
async fn authorized_terminal_response_reads_exact_bytes_and_replays() {
    let fixture = fixture(true, true).await;
    let service = fixture.service();

    let first = service
        .read_response_object(&fixture.read())
        .await
        .expect("first response read");
    let replay = service
        .read_response_object(&fixture.read())
        .await
        .expect("replayed response read");

    assert_eq!(first.reference(), &fixture.reference);
    assert_eq!(first.body(), fixture.body);
    assert_eq!(replay.body(), fixture.body);
    let debug = format!("{first:?}");
    assert!(!debug.contains(&String::from_utf8_lossy(&fixture.body).to_string()));
}

#[tokio::test]
async fn environment_authorization_and_terminal_evidence_precede_object_access() {
    let terminal = fixture(true, true).await;
    let denied = ReadConnectorResponseObject {
        reference: terminal.reference.clone(),
        resource_access: ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
            project_id: terminal.reference.project_id,
            environment_id: EnvironmentId::new(),
        }]),
    };
    assert!(matches!(
        terminal.service().read_response_object(&denied).await,
        Err(ApplicationError::NotFound(_))
    ));

    // Models the object-write/terminal-settlement crash window. Possession of
    // immutable bytes does not authorize consumption without terminal C6 evidence.
    let orphaned = fixture(true, false).await;
    assert!(matches!(
        orphaned
            .service()
            .read_response_object(&orphaned.read())
            .await,
        Err(ApplicationError::NotFound(_))
    ));
}

#[tokio::test]
async fn evidence_drift_missing_and_corrupt_objects_fail_closed() {
    let fixture = fixture(false, true).await;
    assert!(matches!(
        fixture
            .service()
            .read_response_object(&fixture.read())
            .await,
        Err(ApplicationError::Internal(_))
    ));

    let mut drifted = fixture.read();
    drifted.reference = ConnectorResponseObjectReference::new(
        fixture.reference.organization_id,
        fixture.reference.project_id,
        fixture.reference.environment_id,
        fixture.reference.connector_profile_id,
        fixture.reference.connector_revision_id,
        fixture.reference.connector_attempt_id,
        Sha256Digest::from_bytes(b"different"),
        b"different".len() as u64,
    )
    .expect("structurally valid drifted reference");
    assert!(matches!(
        fixture.service().read_response_object(&drifted).await,
        Err(ApplicationError::Conflict(_))
    ));

    let unconfigured = TestResponseObjectPort {
        attempts: fixture.attempts.clone(),
        objects: None,
    };
    assert!(matches!(
        unconfigured.read_response_object(&fixture.read()).await,
        Err(ApplicationError::Unavailable(_))
    ));

    let corrupt: Arc<dyn IConnectorResponseObjectStore> = Arc::new(CorruptResponseObjects);
    let service = TestResponseObjectPort {
        attempts: fixture.attempts.clone(),
        objects: Some(corrupt),
    };
    assert!(matches!(
        service.read_response_object(&fixture.read()).await,
        Err(ApplicationError::Internal(_))
    ));
}

struct CorruptResponseObjects;

#[async_trait]
impl IConnectorResponseObjectStore for CorruptResponseObjects {
    async fn put(
        &self,
        _reference: &ConnectorResponseObjectReference,
        _body: Vec<u8>,
    ) -> Result<ConnectorResponseObjectWrite, ConnectorResponseObjectError> {
        unreachable!("read-only corruption fixture")
    }

    async fn get(
        &self,
        _reference: &ConnectorResponseObjectReference,
    ) -> Result<Vec<u8>, ConnectorResponseObjectError> {
        Err(ConnectorResponseObjectError::Integrity(
            "fixture corruption".into(),
        ))
    }
}

#[test]
fn response_consumption_contracts_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    fn assert_response_port<T: IConnectorResponseObjectPort>() {}

    assert_send_sync::<ReadConnectorResponseObject>();
    assert_send_sync::<super::ConnectorResponseObjectContent>();
    assert_send_sync::<ConnectorExecutionApplicationService>();
    assert_response_port::<ConnectorExecutionApplicationService>();
}
