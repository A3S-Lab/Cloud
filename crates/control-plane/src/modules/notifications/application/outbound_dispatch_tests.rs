use super::*;
use crate::modules::connectors::{
    ConnectorDefinition, ConnectorExecutionError, ConnectorExecutionReceipt,
    ConnectorExecutionRequest, ConnectorExecutionServiceOptions, ConnectorHttpAuthentication,
    ConnectorHttpDefinition, ConnectorHttpDefinitionSpec, ConnectorHttpDestination,
    ConnectorHttpMethod, ConnectorHttpStatusPolicy, ConnectorProfile, ConnectorRecord,
    ConnectorRevision, CreateConnectorProfileWrite, IConnectorExecutionPreparationPort,
    IConnectorProfileRepository, IPreparedConnectorExecution, InMemoryConnectorExecutionRepository,
    InMemoryConnectorProfileRepository,
};
use crate::modules::notifications::{
    Notification, NotificationScope, NotificationSeverity, OutboundNotificationConnectorTarget,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId,
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, ResourceName,
};
use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Fixture {
    dispatcher: OutboundNotificationDispatcher,
    delivery: OutboundNotificationDelivery,
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
                    endpoint: "https://hooks.example.test/notifications".into(),
                },
                method: ConnectorHttpMethod::Post,
                request_content_type: "application/json".into(),
                maximum_request_bytes: 16 * 1024,
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
        ResourceName::parse("Notification Connector").expect("name"),
        &revision,
    )
    .expect("profile");
    let record = ConnectorRecord::new(profile, revision.clone()).expect("record");
    let profiles = Arc::new(InMemoryConnectorProfileRepository::new());
    let request_id = Uuid::now_v7();
    profiles
        .create(CreateConnectorProfileWrite {
            event: crate::modules::connectors::ConnectorRevisionPublished::created(
                &record.profile,
                &record.revision,
                request_id,
            )
            .expect("event"),
            actor_principal_id: revision.created_by,
            request_id,
            idempotency: IdempotencyRequest::new(
                "notification-dispatch-test",
                "connector",
                revision.definition.digest().as_str().as_bytes(),
            )
            .expect("idempotency"),
            record,
        })
        .await
        .expect("store profile");

    let notification = Notification::project(
        revision.organization_id,
        PrincipalId::new(),
        Uuid::now_v7(),
        "identity.membership.role-changed".into(),
        1,
        Uuid::now_v7(),
        2,
        Uuid::now_v7(),
        NotificationSeverity::Warning,
        "Organization role changed".into(),
        "Your organization role is now member.".into(),
        NotificationScope::Organization,
        now,
        now,
    )
    .expect("notification");
    let delivery = OutboundNotificationDelivery::from_notification(
        &notification,
        OutboundNotificationChannel::SlackCompatible,
        OutboundNotificationConnectorTarget::new(
            revision.project_id,
            revision.environment_id,
            revision.profile_id,
            revision.id,
        )
        .expect("target"),
    )
    .expect("delivery");
    let first_attempt = outbound_notification_attempt_id(delivery.id(), 1).expect("first attempt");
    let dispatches = Arc::new(AtomicUsize::new(0));
    let preparation = Arc::new(RecordingSequencePreparation {
        retryable_attempts: Mutex::new(HashSet::from([first_attempt])),
        dispatches: Arc::clone(&dispatches),
    });
    let service = Arc::new(
        ConnectorExecutionApplicationService::new(
            profiles,
            Arc::new(InMemoryConnectorExecutionRepository::new()),
            preparation,
            ConnectorExecutionServiceOptions::default(),
        )
        .expect("Connector service"),
    );
    Fixture {
        dispatcher: OutboundNotificationDispatcher::new(service),
        delivery,
        dispatches,
    }
}

struct RecordingSequencePreparation {
    retryable_attempts: Mutex<HashSet<Uuid>>,
    dispatches: Arc<AtomicUsize>,
}

#[async_trait]
impl IConnectorExecutionPreparationPort for RecordingSequencePreparation {
    async fn prepare(
        &self,
        _revision: &ConnectorRevision,
        request: &ConnectorExecutionRequest,
    ) -> Result<Box<dyn IPreparedConnectorExecution>, ConnectorExecutionError> {
        Ok(Box::new(RecordingSequencePrepared {
            attempt_id: request.attempt_id(),
            retryable: self
                .retryable_attempts
                .lock()
                .expect("retryable attempt lock")
                .contains(&request.attempt_id()),
            dispatches: Arc::clone(&self.dispatches),
        }))
    }
}

struct RecordingSequencePrepared {
    attempt_id: Uuid,
    retryable: bool,
    dispatches: Arc<AtomicUsize>,
}

#[async_trait]
impl IPreparedConnectorExecution for RecordingSequencePrepared {
    fn outcome_timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    async fn dispatch(
        self: Box<Self>,
        request: &ConnectorExecutionRequest,
    ) -> Result<ConnectorExecutionReceipt, ConnectorExecutionError> {
        self.dispatches.fetch_add(1, Ordering::SeqCst);
        if self.retryable {
            return Err(ConnectorExecutionError::Retryable { retry_after: None });
        }
        ConnectorExecutionReceipt::accepted(
            request.connector_revision_id(),
            self.attempt_id,
            canonical_timestamp(Utc::now()),
            204,
            None,
            Vec::new(),
        )
    }
}

#[tokio::test]
async fn event_redelivery_advances_only_past_durable_retryable_evidence() {
    let fixture = fixture().await;
    let first = fixture
        .dispatcher
        .dispatch(&fixture.delivery, 1)
        .await
        .expect("first delivery");
    assert!(matches!(
        first,
        OutboundNotificationDispatchResult::Retryable { generation: 1, .. }
    ));
    assert_eq!(fixture.dispatches.load(Ordering::SeqCst), 1);

    let second = fixture
        .dispatcher
        .dispatch(&fixture.delivery, 2)
        .await
        .expect("redelivery");
    assert!(matches!(
        second,
        OutboundNotificationDispatchResult::Delivered { generation: 2, .. }
    ));
    assert_eq!(fixture.dispatches.load(Ordering::SeqCst), 2);

    let lost_ack = fixture
        .dispatcher
        .dispatch(&fixture.delivery, 3)
        .await
        .expect("lost acknowledgement replay");
    assert!(matches!(
        lost_ack,
        OutboundNotificationDispatchResult::Delivered { generation: 2, .. }
    ));
    assert_eq!(fixture.dispatches.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_delivery_count_jump_does_not_compress_new_provider_retries() {
    let fixture = fixture().await;
    let result = fixture
        .dispatcher
        .dispatch(
            &fixture.delivery,
            MAXIMUM_OUTBOUND_NOTIFICATION_DELIVERY_GENERATION + 1,
        )
        .await
        .expect("delivery after transport-only retries");
    assert!(matches!(
        result,
        OutboundNotificationDispatchResult::Retryable { generation: 1, .. }
    ));
    assert_eq!(fixture.dispatches.load(Ordering::SeqCst), 1);
}

#[test]
fn attempt_generation_is_deterministic_bounded_and_delivery_scoped() {
    let delivery_id = Uuid::now_v7();
    assert_eq!(
        outbound_notification_attempt_id(delivery_id, 7),
        outbound_notification_attempt_id(delivery_id, 7)
    );
    assert_ne!(
        outbound_notification_attempt_id(delivery_id, 7),
        outbound_notification_attempt_id(delivery_id, 8)
    );
    assert!(outbound_notification_attempt_id(Uuid::nil(), 1).is_err());
    assert!(outbound_notification_attempt_id(delivery_id, 0).is_err());
    assert!(outbound_notification_attempt_id(
        delivery_id,
        MAXIMUM_OUTBOUND_NOTIFICATION_DELIVERY_GENERATION + 1
    )
    .is_err());
}
