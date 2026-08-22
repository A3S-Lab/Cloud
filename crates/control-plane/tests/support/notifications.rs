use super::*;
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_cloud_control_plane::modules::connectors::{
    BeginConnectorExecutionDispatch, ConnectorDefinition, ConnectorExecutionAttemptBinding,
    ConnectorExecutionEvidence, ConnectorExecutionReceipt, ConnectorExecutionRequest,
    ConnectorExecutionReservation, ConnectorHttpAuthentication, ConnectorHttpDefinition,
    ConnectorHttpDefinitionSpec, ConnectorHttpDestination, ConnectorHttpMethod,
    ConnectorHttpStatusPolicy, ConnectorProfile, ConnectorRecord, ConnectorRevision,
    ConnectorRevisionPublished, CreateConnectorProfileWrite, IConnectorExecutionAttemptRepository,
    IConnectorProfileRepository, PostgresConnectorExecutionAttemptRepository,
    PostgresConnectorProfileRepository, ReserveConnectorExecutionAttempt,
    SettleConnectorExecutionAttempt,
};
use a3s_cloud_control_plane::modules::edge::domain::events::{
    renewal_subject_id, DomainClaimChanged, GatewayCertificateRenewalChanged,
    GatewayCertificateRenewalFailureKind, GatewayCertificateRenewalStatus,
};
use a3s_cloud_control_plane::modules::edge::domain::DomainClaimState;
use a3s_cloud_control_plane::modules::identity::PostgresIdentityRepository;
use a3s_cloud_control_plane::modules::integration_events::{
    A3sEventPublisher, IIntegrationEventProjector, OutboxMessage, OutboxRelay, OutboxRelayConfig,
    PostgresOutboxRepository,
};
use a3s_cloud_control_plane::modules::notifications::{
    outbound_notification_attempt_id, A3sEventOutboundNotificationConsumer,
    CreateNotificationAlertPolicyWrite, CreateOutboundNotificationSubscriptionWrite,
    INotificationAlertPolicyRepository, INotificationRepository,
    IOutboundNotificationDeliveryRepository, IOutboundNotificationDispatcher,
    IOutboundNotificationRepository, MarkNotificationReadWrite, Notification,
    NotificationAlertPolicy, NotificationAlertPolicyDefinition, NotificationAlertPolicyEvent,
    NotificationAlertPolicySpec, NotificationAlertSource, NotificationScope, NotificationSeverity,
    OutboundNotificationChannel, OutboundNotificationConnectorTarget, OutboundNotificationDelivery,
    OutboundNotificationDeliveryAdmission, OutboundNotificationDispatchResult,
    OutboundNotificationSubscription, OutboundNotificationSubscriptionDefinition,
    OutboundNotificationSubscriptionEvent, OutboundNotificationSubscriptionSpec,
    OutboundNotificationTerminalOutcome, OutboundNotificationTerminalReceipt,
    OutboxNotificationProjector, PostgresNotificationRepository,
    RevokeNotificationAlertPolicyWrite, OUTBOUND_NOTIFICATION_EVENT_KEY,
};
use a3s_cloud_control_plane::modules::shared_kernel::application::{
    ApplicationError, ApplicationResult,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, DomainClaimId, EnvironmentId, GatewayCertificateId,
    IdempotencyRequest, MembershipId, NodeId, NotificationAlertPolicyId,
    NotificationSubscriptionId, OrganizationId, PrincipalId, ProjectId, RepositoryError,
    ResourceName, RouteId, Sha256Digest, WorkloadId,
};
use a3s_event::{Event, NatsConfig, StorageType};
use a3s_orm::{DatabaseError, Executor, PostgresError, PostgresTransaction, Query};
use async_trait::async_trait;
use chrono::Duration as ChronoDuration;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{watch, Notify};

pub(super) async fn exercise_notification_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("106"),
        )
        .await?;
    assert_eq!(
        migration_state,
        (1, "deduplicated notification inbox projections".into())
    );
    let outbound_migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("114"),
        )
        .await?;
    assert_eq!(
        outbound_migration_state,
        (1, "outbound notification subscriptions and receipts".into())
    );
    let attempt_budget_migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("115"),
        )
        .await?;
    assert_eq!(
        attempt_budget_migration_state,
        (1, "bounded outbound notification attempt receipts".into())
    );
    let versioned_budget_migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("128"),
        )
        .await?;
    assert_eq!(
        versioned_budget_migration_state,
        (1, "versioned outbound notification delivery budgets".into())
    );
    let suppression_migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("129"),
        )
        .await?;
    assert_eq!(
        suppression_migration_state,
        (
            1,
            "bounded outbound notification event-time suppression".into()
        )
    );
    let alert_policy_migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("130"),
        )
        .await?;
    assert_eq!(
        alert_policy_migration_state,
        (1, "immutable personal notification alert policies".into())
    );
    let certificate_alert_source_migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("133"),
        )
        .await?;
    assert_eq!(
        certificate_alert_source_migration_state,
        (
            1,
            "Gateway certificate-renewal notification alert source".into()
        )
    );

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let recipient = PrincipalId::new();
    let other_recipient = PrincipalId::new();
    let created_at = Utc::now();
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'Notification tenant', ")
            .bind(format!("notification-{organization_id}"))
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    for (principal_id, name) in [
        (recipient, "Notification recipient"),
        (other_recipient, "Other recipient"),
    ] {
        database
            .execute(
                sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                    .bind(principal_id.as_uuid())
                    .append(", 'human', ")
                    .bind(name)
                    .append(", 1, ")
                    .bind(created_at)
                    .append(", null)"),
            )
            .await?;
    }
    database
        .execute(
            sql_query::<()>(
                "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(project_id.as_uuid())
            .append(", 'Notification delivery', 'notification-delivery', 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into environments (organization_id, project_id, id, name, name_key, aggregate_version, created_at) values (")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(project_id.as_uuid())
                .append(", ")
                .bind(environment_id.as_uuid())
                .append(", 'Production', 'production', 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;

    let connector_repository = PostgresConnectorProfileRepository::new(executor.clone());
    let connector_revision = create_notification_connector(
        &connector_repository,
        organization_id,
        project_id,
        environment_id,
        recipient,
        created_at,
    )
    .await?;
    let target = OutboundNotificationConnectorTarget::new(
        project_id,
        environment_id,
        connector_revision.profile_id,
        connector_revision.id,
    )?;
    let notification_repository = PostgresNotificationRepository::new(executor.clone());
    let subscription = create_outbound_subscription(
        &notification_repository,
        organization_id,
        recipient,
        target,
        created_at,
    )
    .await?;
    assert_null_suppression_rejected(&database, &subscription).await?;

    let suppress_before = subscription
        .definition
        .suppress_before()
        .ok_or("version three subscription must retain its event-time cutoff")?;
    assert_eq!(
        suppress_before,
        subscription.created_at + ChronoDuration::seconds(1)
    );
    let suppressed = source_notification(
        &database,
        organization_id,
        recipient,
        "Suppressed outbound notification",
        suppress_before - ChronoDuration::microseconds(1),
    )
    .await?;
    let repository = notification_repository;
    assert!(repository.project(suppressed.clone()).await?);
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from notification_outbound_deliveries where organization_id = ",
                )
                .bind(organization_id.as_uuid()),
            )
            .await?,
        0
    );
    let forged_suppressed_delivery = subscription.definition.delivery_for(&suppressed)?;
    assert_suppressed_delivery_rejected(&executor, &forged_suppressed_delivery, subscription.id)
        .await?;

    let first = source_notification(
        &database,
        organization_id,
        recipient,
        "Organization access granted",
        suppress_before,
    )
    .await?;
    assert!(repository.project(first.clone()).await?);
    assert!(!repository.project(first.clone()).await?);
    assert_eq!(subscription.definition.maximum_provider_attempts(), 2);
    let outbound_delivery = subscription.definition.delivery_for(&first)?;
    assert!(matches!(
        repository.admit_delivery(&outbound_delivery).await?,
        Some(OutboundNotificationDeliveryAdmission::Pending)
    ));
    let atomic_delivery = database
        .fetch_one_as(
            sql_query::<(i64, i64)>(
                "select (select count(*) from notification_outbound_deliveries where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and id = ")
            .bind(outbound_delivery.id())
            .append("), (select count(*) from outbox_events where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and event_id = ")
            .bind(outbound_delivery.requested_event_id())
            .append(" and event_key = 'notification.delivery.requested')"),
        )
        .await?;
    assert_eq!(atomic_delivery, (1, 1));

    let attempt_id = outbound_notification_attempt_id(outbound_delivery.id(), 1)?;
    let connector_request = ConnectorExecutionRequest::new(
        connector_revision.id,
        attempt_id,
        "application/json",
        b"notification delivery fixture".to_vec(),
    )?;
    let dispatch_started_at = created_at + ChronoDuration::seconds(4);
    let connector_receipt = ConnectorExecutionReceipt::accepted(
        connector_revision.id,
        attempt_id,
        created_at + ChronoDuration::seconds(5),
        204,
        None,
        Vec::new(),
    )?;
    let connector_evidence = ConnectorExecutionEvidence::accepted(
        &connector_revision,
        &connector_request,
        &connector_receipt,
        dispatch_started_at,
    )?;
    let attempts = PostgresConnectorExecutionAttemptRepository::new(executor.clone());
    persist_connector_evidence(
        &attempts,
        &connector_revision,
        &connector_request,
        connector_evidence.clone(),
    )
    .await?;
    let terminal_receipt =
        OutboundNotificationTerminalReceipt::delivered(&outbound_delivery, 1, &connector_evidence)?;
    assert!(
        repository
            .settle_delivery(&outbound_delivery, terminal_receipt.clone())
            .await?
    );
    assert!(
        !repository
            .settle_delivery(&outbound_delivery, terminal_receipt.clone())
            .await?
    );
    assert_eq!(
        repository.admit_delivery(&outbound_delivery).await?,
        Some(OutboundNotificationDeliveryAdmission::Terminal(
            terminal_receipt
        ))
    );
    let mut drift = first.clone();
    drift.title = "Changed title".into();
    assert!(repository.project(drift).await.is_err());

    let concurrent = source_notification(
        &database,
        organization_id,
        recipient,
        "Organization role changed",
        created_at + ChronoDuration::seconds(2),
    )
    .await?;
    let (left, right) = tokio::join!(
        repository.project(concurrent.clone()),
        repository.project(concurrent.clone())
    );
    let outcomes = [left?, right?];
    assert_eq!(outcomes.into_iter().filter(|inserted| *inserted).count(), 1);

    let exhausted_delivery = subscription.definition.delivery_for(&concurrent)?;
    let maximum_provider_attempts = subscription.definition.maximum_provider_attempts();
    let exhausted_attempt_id =
        outbound_notification_attempt_id(exhausted_delivery.id(), maximum_provider_attempts)?;
    let exhausted_request = ConnectorExecutionRequest::new(
        connector_revision.id,
        exhausted_attempt_id,
        "application/json",
        b"exhausted notification delivery fixture".to_vec(),
    )?;
    let exhausted_started_at = created_at + ChronoDuration::seconds(7);
    let exhausted_evidence = ConnectorExecutionEvidence::retryable(
        &connector_revision,
        &exhausted_request,
        Some(503),
        Some(std::time::Duration::from_secs(60)),
        exhausted_started_at,
        created_at + ChronoDuration::seconds(8),
    )?;
    persist_connector_evidence(
        &attempts,
        &connector_revision,
        &exhausted_request,
        exhausted_evidence.clone(),
    )
    .await?;
    let exhausted_receipt = OutboundNotificationTerminalReceipt::exhausted(
        &exhausted_delivery,
        maximum_provider_attempts,
        &exhausted_evidence,
    )?;
    assert!(
        repository
            .settle_delivery(&exhausted_delivery, exhausted_receipt.clone())
            .await?
    );
    assert_eq!(
        repository.admit_delivery(&exhausted_delivery).await?,
        Some(OutboundNotificationDeliveryAdmission::Terminal(
            exhausted_receipt
        ))
    );

    assert!(repository
        .find(organization_id, other_recipient, first.id)
        .await?
        .is_none());
    assert_eq!(
        repository
            .list_page(organization_id, recipient, false, None, 50)
            .await?
            .len(),
        3
    );

    let request_id = Uuid::now_v7();
    let read = first.mark_read(1, created_at + ChronoDuration::seconds(6))?;
    let idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{organization_id}/notifications/{}/read",
            first.id
        ),
        "postgres:notification:read",
        b"expected-version:1",
    )?;
    let event = DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: "notification.inbox.read".into(),
        schema_version: 1,
        organization_id: organization_id.as_uuid(),
        aggregate_id: read.id.as_uuid(),
        aggregate_version: read.aggregate_version,
        occurred_at: read.read_at.expect("read time"),
        correlation_id: request_id,
        causation_id: Some(read.source_event_id),
        payload: json!({
            "notificationId": read.id,
            "sourceEventId": read.source_event_id
        }),
    };
    let write = MarkNotificationReadWrite {
        notification: read.clone(),
        expected_version: 1,
        actor_principal_id: recipient,
        event,
        idempotency: idempotency.clone(),
        request_id,
    };
    let written = repository.mark_read(write.clone()).await?;
    assert!(!written.replayed);
    let replayed = repository.mark_read(write).await?;
    assert!(replayed.replayed);
    assert_eq!(replayed.value, read);
    assert!(repository
        .replay_mark_read(&idempotency)
        .await?
        .is_some_and(|value| value.replayed && value.value == read));
    assert_eq!(
        repository
            .list_page(organization_id, recipient, true, None, 50)
            .await?
            .len(),
        2
    );

    assert_rejected(
        database
            .execute(
                sql_query::<()>(
                    "update notifications set title = 'Tampered' where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(first.id.as_uuid()),
            )
            .await,
        "mutate immutable notification content",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("delete from notifications where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(first.id.as_uuid()),
            )
            .await,
        "delete a notification",
    );

    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64)>("select (select count(*) from notifications where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and recipient_principal_id = ")
                .bind(recipient.as_uuid())
                .append("), (select count(*) from notifications where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and recipient_principal_id = ")
                .bind(recipient.as_uuid())
                .append(" and read_at is not null), (select count(*) from outbox_events where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and event_key = 'notification.inbox.read'), (select count(*) from audit_records where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and action = 'notification.inbox.read')"),
        )
        .await?;
    assert_eq!(evidence, (3, 1, 1, 1));
    let outbound_evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64)>("select (select count(*) from notification_outbound_subscriptions where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from notification_outbound_deliveries where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from notification_outbound_deliveries where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and terminal_outcome = 'delivered'), (select count(*) from notification_outbound_deliveries where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and terminal_outcome = 'exhausted')"),
        )
        .await?;
    assert_eq!(outbound_evidence, (1, 2, 1, 1));
    let pinned_budget_evidence = database
        .fetch_one_as(
            sql_query::<(String, i64, i64, i32, chrono::DateTime<Utc>)>(
                "select (select min(definition_schema) from notification_outbound_subscriptions where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append("), (select min(maximum_provider_attempts) from notification_outbound_subscriptions where organization_id = ")
            .bind(organization_id.as_uuid())
            .append("), (select min(maximum_provider_attempts) from notification_outbound_deliveries where organization_id = ")
            .bind(organization_id.as_uuid())
            .append("), (select min(schema_version) from outbox_events where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and event_key = 'notification.delivery.requested'), (select min(suppress_before) from notification_outbound_subscriptions where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(")"),
        )
        .await?;
    assert_eq!(
        pinned_budget_evidence,
        (
            "cloud.notification.outbound-subscription.v3".into(),
            2,
            2,
            2,
            suppress_before
        )
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>(
                    "update notification_outbound_subscriptions set maximum_provider_attempts = 3 where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(subscription.id.as_uuid()),
            )
            .await,
        "mutate a subscription provider-attempt budget",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>(
                    "update notification_outbound_subscriptions set suppress_before = suppress_before + interval '1 second' where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(subscription.id.as_uuid()),
            )
            .await,
        "mutate a subscription event-time cutoff",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>(
                    "update notification_outbound_deliveries set maximum_provider_attempts = 3 where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(outbound_delivery.id()),
            )
            .await,
        "mutate a delivery provider-attempt budget",
    );

    exercise_notification_alert_policy_persistence(
        executor.clone(),
        &database,
        organization_id,
        project_id,
        environment_id,
        other_recipient,
        created_at + ChronoDuration::seconds(10),
    )
    .await?;

    if let Ok(nats_url) = std::env::var("A3S_CLOUD_TEST_NATS_URL") {
        exercise_notification_nats_delivery(
            nats_url,
            executor,
            &database,
            &repository,
            organization_id,
            recipient,
            connector_revision,
            subscription.definition.clone(),
            created_at + ChronoDuration::seconds(20),
        )
        .await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn exercise_notification_alert_policy_persistence(
    executor: PostgresExecutor,
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    recipient: PrincipalId,
    created_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let membership_id = MembershipId::new();
    database
        .execute(
            sql_query::<()>("insert into organization_memberships (id, organization_id, principal_id, role, aggregate_version, created_at, updated_at, revoked_at) values (")
                .bind(membership_id.as_uuid())
                .append(", ")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(recipient.as_uuid())
                .append(", 'member', 1, ")
                .bind(created_at)
                .append(", ")
                .bind(created_at)
                .append(", null)"),
        )
        .await?;

    let repository = Arc::new(PostgresNotificationRepository::new(executor.clone()));
    let definition = NotificationAlertPolicyDefinition::from_spec(NotificationAlertPolicySpec {
        source: NotificationAlertSource::EdgeDomainClaimStatusV1,
        project_id,
        environment_id,
        notify_on_recovery: true,
    })?;
    let policy = NotificationAlertPolicy::create(
        organization_id,
        NotificationAlertPolicyId::new(),
        recipient,
        definition.clone(),
        recipient,
        created_at,
    )?;
    let create_write = notification_alert_policy_create_write(&policy, "postgres:alert:create")?;
    let created = repository.create_alert_policy(create_write.clone()).await?;
    assert!(!created.replayed);
    assert_eq!(created.value, policy);
    let replayed = repository.create_alert_policy(create_write).await?;
    assert!(replayed.replayed);
    assert_eq!(replayed.value, policy);
    assert_eq!(
        repository
            .find_alert_policy(organization_id, recipient, policy.id)
            .await?,
        Some(policy.clone())
    );
    assert_eq!(
        repository
            .list_alert_policy_page(organization_id, recipient, None, 50)
            .await?,
        vec![policy.clone()]
    );
    assert_eq!(
        repository
            .list_active_alert_policies_for_source(
                organization_id,
                NotificationAlertSource::EdgeDomainClaimStatusV1,
                project_id,
                environment_id,
                policy.created_at,
            )
            .await?,
        vec![policy.clone()]
    );

    let duplicate = NotificationAlertPolicy::create(
        organization_id,
        NotificationAlertPolicyId::new(),
        recipient,
        definition,
        recipient,
        policy.created_at,
    )?;
    assert!(matches!(
        repository
            .create_alert_policy(notification_alert_policy_create_write(
                &duplicate,
                "postgres:alert:duplicate",
            )?)
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let certificate_definition =
        NotificationAlertPolicyDefinition::from_spec(NotificationAlertPolicySpec {
            source: NotificationAlertSource::EdgeGatewayCertificateRenewalStatusV1,
            project_id,
            environment_id,
            notify_on_recovery: true,
        })?;
    let certificate_policy = NotificationAlertPolicy::create(
        organization_id,
        NotificationAlertPolicyId::new(),
        recipient,
        certificate_definition,
        recipient,
        policy.created_at + ChronoDuration::milliseconds(1),
    )?;
    let certificate_create_write = notification_alert_policy_create_write(
        &certificate_policy,
        "postgres:certificate-alert:create",
    )?;
    let certificate_created = repository
        .create_alert_policy(certificate_create_write.clone())
        .await?;
    assert!(!certificate_created.replayed);
    assert_eq!(certificate_created.value, certificate_policy);
    assert!(
        repository
            .create_alert_policy(certificate_create_write)
            .await?
            .replayed
    );
    assert_eq!(
        repository
            .list_active_alert_policies_for_source(
                organization_id,
                NotificationAlertSource::EdgeGatewayCertificateRenewalStatusV1,
                project_id,
                environment_id,
                certificate_policy.created_at,
            )
            .await?,
        vec![certificate_policy.clone()]
    );
    assert_eq!(
        repository
            .list_alert_policy_page(organization_id, recipient, None, 50)
            .await?
            .len(),
        2
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("insert into notification_alert_policies (organization_id, id, recipient_principal_id, source, project_id, environment_id, notify_on_recovery, definition_schema, canonical_acl, definition_digest, aggregate_version, created_by, created_at, revoked_at) select organization_id, ")
                    .bind(Uuid::now_v7())
                    .append(", recipient_principal_id, 'edge.unreviewed-event.v1', project_id, environment_id, notify_on_recovery, definition_schema, canonical_acl, definition_digest, aggregate_version, created_by, created_at, revoked_at from notification_alert_policies where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(certificate_policy.id.as_uuid()),
            )
            .await,
        "persist an unregistered notification alert source",
    );

    let identity = Arc::new(PostgresIdentityRepository::new(executor));
    let projector = OutboxNotificationProjector::new(repository.clone(), identity.clone())
        .with_alert_policies(repository.clone(), identity);
    let claim_id = DomainClaimId::new();
    let rejected = notification_domain_claim_message(
        organization_id,
        project_id,
        environment_id,
        claim_id,
        "edge.domain-claim.rejected",
        DomainClaimState::Rejected,
        Some("provider-private rejection detail"),
        2,
        policy.created_at + ChronoDuration::seconds(1),
    )?;
    persist_outbox_message(database, &rejected).await?;
    projector.project(&rejected).await?;
    projector.project(&rejected).await?;
    let projected = repository
        .list_page(organization_id, recipient, false, None, 50)
        .await?;
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].severity, NotificationSeverity::Warning);
    assert_eq!(projected[0].source_event_key, "edge.domain-claim.rejected");
    assert!(!projected[0].body.contains("provider-private"));

    let recovered = notification_domain_claim_message(
        organization_id,
        project_id,
        environment_id,
        claim_id,
        "edge.domain-claim.verified",
        DomainClaimState::Verified,
        None,
        3,
        policy.created_at + ChronoDuration::seconds(2),
    )?;
    persist_outbox_message(database, &recovered).await?;
    projector.project(&recovered).await?;
    projector.project(&recovered).await?;
    let projected = repository
        .list_page(organization_id, recipient, false, None, 50)
        .await?;
    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].source_event_key, "edge.domain-claim.verified");
    assert_eq!(projected[0].severity, NotificationSeverity::Information);
    assert_eq!(projected[1].source_event_key, "edge.domain-claim.rejected");

    let revoked = policy.revoke(1, recipient, policy.created_at + ChronoDuration::seconds(3))?;
    let revoke_request_id = Uuid::now_v7();
    let revoke_write = RevokeNotificationAlertPolicyWrite {
        event: NotificationAlertPolicyEvent::envelope(
            "notification.alert-policy.revoked",
            &revoked,
            revoke_request_id,
        )?,
        policy: revoked.clone(),
        expected_version: 1,
        actor_principal_id: recipient,
        request_id: revoke_request_id,
        idempotency: IdempotencyRequest::new(
            "notification-alert-policy-revoke",
            "postgres:alert:revoke",
            b"expected-version:1",
        )?,
    };
    let revoked_write = repository.revoke_alert_policy(revoke_write.clone()).await?;
    assert!(!revoked_write.replayed);
    assert_eq!(revoked_write.value, revoked);
    assert!(repository.revoke_alert_policy(revoke_write).await?.replayed);
    assert!(repository
        .list_active_alert_policies_for_source(
            organization_id,
            NotificationAlertSource::EdgeDomainClaimStatusV1,
            project_id,
            environment_id,
            revoked.revoked_at.expect("revoked at"),
        )
        .await?
        .is_empty());

    let late_rejected = notification_domain_claim_message(
        organization_id,
        project_id,
        environment_id,
        DomainClaimId::new(),
        "edge.domain-claim.rejected",
        DomainClaimState::Rejected,
        Some("late rejection"),
        2,
        revoked.revoked_at.expect("revoked at") + ChronoDuration::seconds(1),
    )?;
    persist_outbox_message(database, &late_rejected).await?;
    projector.project(&late_rejected).await?;
    assert_eq!(
        repository
            .list_page(organization_id, recipient, false, None, 50)
            .await?
            .len(),
        2
    );

    let route_id = RouteId::new();
    let node_id = NodeId::new();
    let initial_renewal = notification_gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        node_id,
        "edge.gateway-certificate.renewed",
        GatewayCertificateRenewalStatus::Renewed,
        None,
        10,
        policy.created_at + ChronoDuration::seconds(5),
    )?;
    persist_outbox_message(database, &initial_renewal).await?;
    projector.project(&initial_renewal).await?;

    let unavailable = notification_gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        node_id,
        "edge.gateway-certificate.renewal-failed",
        GatewayCertificateRenewalStatus::Failed,
        Some(GatewayCertificateRenewalFailureKind::Unavailable),
        11,
        policy.created_at + ChronoDuration::seconds(6),
    )?;
    persist_outbox_message(database, &unavailable).await?;
    projector.project(&unavailable).await?;
    projector.project(&unavailable).await?;

    let peer_recovery = notification_gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        NodeId::new(),
        "edge.gateway-certificate.renewed",
        GatewayCertificateRenewalStatus::Renewed,
        None,
        12,
        policy.created_at + ChronoDuration::seconds(7),
    )?;
    persist_outbox_message(database, &peer_recovery).await?;
    projector.project(&peer_recovery).await?;

    let renewed = notification_gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        node_id,
        "edge.gateway-certificate.renewed",
        GatewayCertificateRenewalStatus::Renewed,
        None,
        12,
        policy.created_at + ChronoDuration::seconds(8),
    )?;
    persist_outbox_message(database, &renewed).await?;
    projector.project(&renewed).await?;
    projector.project(&renewed).await?;

    let certificate_notifications = repository
        .list_page(organization_id, recipient, false, None, 50)
        .await?
        .into_iter()
        .filter(|notification| {
            notification
                .source_event_key
                .starts_with("edge.gateway-certificate.")
        })
        .collect::<Vec<_>>();
    assert_eq!(certificate_notifications.len(), 2);
    assert_eq!(
        certificate_notifications[0].source_event_key,
        "edge.gateway-certificate.renewed"
    );
    assert_eq!(
        certificate_notifications[0].severity,
        NotificationSeverity::Information
    );
    assert_eq!(
        certificate_notifications[1].source_event_key,
        "edge.gateway-certificate.renewal-failed"
    );
    assert_eq!(
        certificate_notifications[1].severity,
        NotificationSeverity::Critical
    );
    assert!(certificate_notifications.iter().all(|notification| {
        notification.scope
            == NotificationScope::Environment {
                project_id,
                environment_id,
            }
            && notification.body.contains("postgres-tls.example.com")
            && notification.body.contains(&route_id.to_string())
            && notification.body.contains(&node_id.to_string())
    }));

    assert_rejected(
        database
            .execute(
                sql_query::<()>(
                    "update notification_alert_policies set canonical_acl = canonical_acl || ' ' where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(policy.id.as_uuid()),
            )
            .await,
        "mutate a notification alert policy ACL",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("delete from notification_alert_policies where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(policy.id.as_uuid()),
            )
            .await,
        "delete a notification alert policy",
    );
    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64)>(
                "select (select count(*) from notification_alert_policies where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and recipient_principal_id = ")
            .bind(recipient.as_uuid())
            .append("), (select count(*) from outbox_events where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and event_key = 'notification.alert-policy.created'), (select count(*) from outbox_events where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and event_key = 'notification.alert-policy.revoked'), (select count(*) from audit_records where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and action in ('notification.alert-policy.created', 'notification.alert-policy.revoked')), (select count(*) from notifications where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and recipient_principal_id = ")
            .bind(recipient.as_uuid())
            .append(" and source_event_key in ('edge.domain-claim.rejected', 'edge.domain-claim.verified', 'edge.gateway-certificate.renewal-failed', 'edge.gateway-certificate.renewed'))"),
        )
        .await?;
    assert_eq!(evidence, (2, 2, 1, 3, 4));
    Ok(())
}

fn notification_alert_policy_create_write(
    policy: &NotificationAlertPolicy,
    idempotency_key: &str,
) -> Result<CreateNotificationAlertPolicyWrite, Box<dyn std::error::Error>> {
    let request_id = Uuid::now_v7();
    Ok(CreateNotificationAlertPolicyWrite {
        event: NotificationAlertPolicyEvent::envelope(
            "notification.alert-policy.created",
            policy,
            request_id,
        )?,
        policy: policy.clone(),
        actor_principal_id: policy.recipient_principal_id,
        request_id,
        idempotency: IdempotencyRequest::new(
            "notification-alert-policy-create",
            idempotency_key,
            policy.definition.digest().as_str().as_bytes(),
        )?,
    })
}

#[allow(clippy::too_many_arguments)]
fn notification_domain_claim_message(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    claim_id: DomainClaimId,
    event_key: &str,
    state: DomainClaimState,
    failure: Option<&str>,
    aggregate_version: u64,
    occurred_at: chrono::DateTime<Utc>,
) -> Result<OutboxMessage, Box<dyn std::error::Error>> {
    Ok(OutboxMessage {
        event_id: Uuid::now_v7(),
        event_key: event_key.into(),
        schema_version: 1,
        organization_id: organization_id.as_uuid(),
        aggregate_id: claim_id.as_uuid(),
        aggregate_version,
        occurred_at,
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        payload: serde_json::to_value(DomainClaimChanged {
            organization_id,
            project_id,
            environment_id,
            domain_claim_id: claim_id,
            pattern: "postgres.example.com".into(),
            state,
            failure: failure.map(str::to_owned),
        })?,
        delivery_attempts: 1,
    })
}

#[allow(clippy::too_many_arguments)]
fn notification_gateway_certificate_renewal_message(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    route_id: RouteId,
    node_id: NodeId,
    event_key: &str,
    status: GatewayCertificateRenewalStatus,
    failure_kind: Option<GatewayCertificateRenewalFailureKind>,
    aggregate_version: u64,
    occurred_at: chrono::DateTime<Utc>,
) -> Result<OutboxMessage, Box<dyn std::error::Error>> {
    let previous_certificate_id = GatewayCertificateId::new();
    let replacement_certificate_id = GatewayCertificateId::new();
    let active_certificate_id = match status {
        GatewayCertificateRenewalStatus::Failed => previous_certificate_id,
        GatewayCertificateRenewalStatus::Renewed => replacement_certificate_id,
    };
    let raw_expiry = occurred_at + ChronoDuration::days(30);
    let active_certificate_expires_at = raw_expiry
        - ChronoDuration::nanoseconds(i64::from(raw_expiry.timestamp_subsec_nanos() % 1_000));
    Ok(OutboxMessage {
        event_id: Uuid::now_v7(),
        event_key: event_key.into(),
        schema_version: 1,
        organization_id: organization_id.as_uuid(),
        aggregate_id: renewal_subject_id(route_id, node_id),
        aggregate_version,
        occurred_at,
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        payload: serde_json::to_value(GatewayCertificateRenewalChanged {
            organization_id,
            project_id,
            environment_id,
            route_id,
            workload_id: WorkloadId::new(),
            node_id,
            hostname: "postgres-tls.example.com".into(),
            path_prefix: "/service".into(),
            gateway_revision: aggregate_version,
            previous_certificate_id,
            replacement_certificate_id,
            active_certificate_id,
            active_certificate_expires_at,
            status,
            failure_kind,
        })?,
        delivery_attempts: 1,
    })
}

async fn persist_outbox_message(
    database: &Database<PostgresDialect, PostgresExecutor>,
    message: &OutboxMessage,
) -> Result<(), Box<dyn std::error::Error>> {
    database
        .execute(
            sql_query::<()>("insert into outbox_events (event_id, event_key, schema_version, organization_id, aggregate_id, aggregate_version, occurred_at, correlation_id, causation_id, payload) values (")
                .bind(message.event_id)
                .append(", ")
                .bind(message.event_key.clone())
                .append(", ")
                .bind(message.schema_version)
                .append(", ")
                .bind(message.organization_id)
                .append(", ")
                .bind(message.aggregate_id)
                .append(", ")
                .bind(message.aggregate_version)
                .append(", ")
                .bind(message.occurred_at)
                .append(", ")
                .bind(message.correlation_id)
                .append(", ")
                .bind(message.causation_id)
                .append(", ")
                .bind(message.payload.clone())
                .append(")"),
        )
        .await?;
    Ok(())
}

struct CountingDeliveryRepository {
    inner: PostgresNotificationRepository,
    admissions: AtomicUsize,
    admission_changed: Notify,
}

impl CountingDeliveryRepository {
    fn new(inner: PostgresNotificationRepository) -> Self {
        Self {
            inner,
            admissions: AtomicUsize::new(0),
            admission_changed: Notify::new(),
        }
    }

    fn admission_count(&self) -> usize {
        self.admissions.load(Ordering::SeqCst)
    }

    async fn wait_for_admission_after(&self, previous: usize) {
        loop {
            let changed = self.admission_changed.notified();
            if self.admission_count() > previous {
                return;
            }
            changed.await;
        }
    }
}

#[async_trait]
impl IOutboundNotificationDeliveryRepository for CountingDeliveryRepository {
    async fn admit_delivery(
        &self,
        delivery: &OutboundNotificationDelivery,
    ) -> Result<Option<OutboundNotificationDeliveryAdmission>, RepositoryError> {
        let result = self.inner.admit_delivery(delivery).await;
        self.admissions.fetch_add(1, Ordering::SeqCst);
        self.admission_changed.notify_waiters();
        result
    }

    async fn settle_delivery(
        &self,
        delivery: &OutboundNotificationDelivery,
        receipt: OutboundNotificationTerminalReceipt,
    ) -> Result<bool, RepositoryError> {
        self.inner.settle_delivery(delivery, receipt).await
    }
}

struct NatsEvidenceDispatcher {
    attempts: PostgresConnectorExecutionAttemptRepository,
    revision: ConnectorRevision,
    calls: AtomicUsize,
}

impl NatsEvidenceDispatcher {
    fn new(executor: PostgresExecutor, revision: ConnectorRevision) -> Self {
        Self {
            attempts: PostgresConnectorExecutionAttemptRepository::new(executor),
            revision,
            calls: AtomicUsize::new(0),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl IOutboundNotificationDispatcher for NatsEvidenceDispatcher {
    async fn dispatch(
        &self,
        delivery: &OutboundNotificationDelivery,
        delivery_count: u64,
    ) -> ApplicationResult<OutboundNotificationDispatchResult> {
        if delivery_count == 0 {
            return Err(ApplicationError::Invalid(
                "NATS delivery count must be positive".into(),
            ));
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        let generation = 1;
        let attempt_id = outbound_notification_attempt_id(delivery.id(), generation)
            .map_err(ApplicationError::Invalid)?;
        let request = ConnectorExecutionRequest::new(
            self.revision.id,
            attempt_id,
            "application/json",
            b"notification NATS request".to_vec(),
        )
        .map_err(ApplicationError::Invalid)?;
        let started_at = delivery.occurred_at() + ChronoDuration::milliseconds(1);
        let receipt = ConnectorExecutionReceipt::accepted(
            self.revision.id,
            attempt_id,
            delivery.occurred_at() + ChronoDuration::milliseconds(2),
            204,
            None,
            Vec::new(),
        )
        .map_err(|error| ApplicationError::Invalid(error.to_string()))?;
        let evidence =
            ConnectorExecutionEvidence::accepted(&self.revision, &request, &receipt, started_at)
                .map_err(|error| ApplicationError::Invalid(error.to_string()))?;
        persist_connector_evidence(&self.attempts, &self.revision, &request, evidence.clone())
            .await
            .map_err(|error| {
                ApplicationError::Internal(format!(
                    "persist NATS notification Connector evidence: {error}"
                ))
            })?;
        Ok(OutboundNotificationDispatchResult::Delivered {
            generation,
            evidence,
        })
    }
}

#[allow(clippy::too_many_arguments)]
async fn exercise_notification_nats_delivery(
    nats_url: String,
    executor: PostgresExecutor,
    database: &Database<PostgresDialect, PostgresExecutor>,
    repository: &PostgresNotificationRepository,
    organization_id: OrganizationId,
    recipient: PrincipalId,
    connector_revision: ConnectorRevision,
    definition: OutboundNotificationSubscriptionDefinition,
    occurred_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let notification = source_notification(
        database,
        organization_id,
        recipient,
        "NATS durable notification",
        occurred_at,
    )
    .await?;
    assert!(repository.project(notification.clone()).await?);
    let delivery = definition.delivery_for(&notification)?;

    let nats_config = NatsConfig {
        url: nats_url,
        stream_name: format!("A3S_CLOUD_NOTIFICATION_{}", Uuid::new_v4().simple()).to_uppercase(),
        subject_prefix: format!("a3s-cloud-notification-{}", Uuid::new_v4().simple())
            .to_lowercase(),
        storage: StorageType::Memory,
        ..NatsConfig::default()
    };
    let publisher = Arc::new(A3sEventPublisher::nats(nats_config).await?);
    let bus = publisher.bus();
    let subject = publisher.subject(OUTBOUND_NOTIFICATION_EVENT_KEY);
    let counting_repository = Arc::new(CountingDeliveryRepository::new(repository.clone()));
    let delivery_repository: Arc<dyn IOutboundNotificationDeliveryRepository> =
        counting_repository.clone();
    let dispatcher = Arc::new(NatsEvidenceDispatcher::new(
        executor.clone(),
        connector_revision,
    ));
    let dispatcher_port: Arc<dyn IOutboundNotificationDispatcher> = dispatcher.clone();

    let (shutdown, consumer_task) = start_notification_consumer(
        Arc::clone(&bus),
        subject.clone(),
        Arc::clone(&delivery_repository),
        Arc::clone(&dispatcher_port),
    )?;
    let relay = OutboxRelay::new(
        Arc::new(PostgresOutboxRepository::new(executor)),
        publisher,
        OutboxRelayConfig {
            batch_size: 1_000,
            poll_interval: std::time::Duration::from_millis(10),
            lease_duration: std::time::Duration::from_secs(5),
            publish_timeout: std::time::Duration::from_secs(2),
            initial_backoff: std::time::Duration::from_millis(10),
            maximum_backoff: std::time::Duration::from_millis(100),
        },
    )?;
    let report = relay.run_once().await?;
    assert!(report.claimed > 0);
    assert!(
        report.failures.is_empty(),
        "NATS Outbox relay failures: {report:?}"
    );

    let receipt = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match repository.admit_delivery(&delivery).await {
                Ok(Some(OutboundNotificationDeliveryAdmission::Terminal(receipt))) => {
                    break Ok::<_, RepositoryError>(receipt);
                }
                Ok(_) => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
                Err(error) => break Err(error),
            }
        }
    })
    .await??;
    assert_eq!(
        receipt.outcome(),
        OutboundNotificationTerminalOutcome::Delivered
    );
    assert_eq!(dispatcher.call_count(), 1);
    stop_notification_consumer(shutdown, consumer_task).await?;

    let admissions_before_replay = counting_repository.admission_count();
    let (shutdown, consumer_task) = start_notification_consumer(
        Arc::clone(&bus),
        subject.clone(),
        delivery_repository,
        dispatcher_port,
    )?;
    bus.publish_event(&delivery_event(&delivery, &subject)?)
        .await?;
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        counting_repository.wait_for_admission_after(admissions_before_replay),
    )
    .await?;
    assert_eq!(dispatcher.call_count(), 1);
    assert!(!consumer_task.is_finished());
    stop_notification_consumer(shutdown, consumer_task).await?;
    assert_eq!(bus.info().await?.consumers, 1);
    Ok(())
}

type NotificationConsumerTask = tokio::task::JoinHandle<a3s_event::Result<()>>;

fn start_notification_consumer(
    bus: Arc<a3s_event::EventBus>,
    subject: String,
    deliveries: Arc<dyn IOutboundNotificationDeliveryRepository>,
    dispatcher: Arc<dyn IOutboundNotificationDispatcher>,
) -> Result<(watch::Sender<bool>, NotificationConsumerTask), Box<dyn std::error::Error>> {
    let consumer = A3sEventOutboundNotificationConsumer::new(bus, subject, deliveries, dispatcher)?;
    let (shutdown, receiver) = watch::channel(false);
    Ok((shutdown, tokio::spawn(consumer.run(receiver))))
}

async fn stop_notification_consumer(
    shutdown: watch::Sender<bool>,
    task: NotificationConsumerTask,
) -> Result<(), Box<dyn std::error::Error>> {
    shutdown.send(true)?;
    tokio::time::timeout(std::time::Duration::from_secs(5), task).await???;
    Ok(())
}

fn delivery_event(delivery: &OutboundNotificationDelivery, subject: &str) -> Result<Event, String> {
    let fact = delivery.requested_event()?;
    let mut event = Event::typed(
        subject,
        "cloud",
        OUTBOUND_NOTIFICATION_EVENT_KEY,
        fact.schema_version,
        OUTBOUND_NOTIFICATION_EVENT_KEY,
        "a3s-cloud",
        json!({
            "organizationId": fact.organization_id,
            "aggregateId": fact.aggregate_id,
            "aggregateVersion": fact.aggregate_version,
            "occurredAt": fact.occurred_at,
            "correlationId": fact.correlation_id,
            "causationId": fact.causation_id,
            "data": fact.payload,
        }),
    );
    event.id = fact.event_id.to_string();
    Ok(event)
}

async fn persist_connector_evidence(
    attempts: &PostgresConnectorExecutionAttemptRepository,
    revision: &ConnectorRevision,
    request: &ConnectorExecutionRequest,
    evidence: ConnectorExecutionEvidence,
) -> Result<(), Box<dyn std::error::Error>> {
    let reserved_at = evidence.started_at() - ChronoDuration::milliseconds(1);
    let fence = match attempts
        .reserve(ReserveConnectorExecutionAttempt::new(
            ConnectorExecutionAttemptBinding::from_exact(revision, request)?,
            Uuid::now_v7(),
            reserved_at,
            reserved_at + ChronoDuration::seconds(30),
        )?)
        .await?
    {
        ConnectorExecutionReservation::Acquired { fence, .. } => fence,
        other => return Err(format!("unexpected Connector reservation: {other:?}").into()),
    };
    attempts
        .begin_dispatch(BeginConnectorExecutionDispatch::new(
            fence.clone(),
            evidence.started_at(),
            evidence.started_at() + ChronoDuration::seconds(60),
        )?)
        .await?;
    attempts
        .settle(SettleConnectorExecutionAttempt::new(fence, evidence)?)
        .await?;
    Ok(())
}

async fn create_notification_connector(
    repository: &PostgresConnectorProfileRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    actor: PrincipalId,
    created_at: chrono::DateTime<Utc>,
) -> Result<ConnectorRevision, Box<dyn std::error::Error>> {
    let profile_id = ConnectorProfileId::new();
    let revision = ConnectorRevision::initial(
        organization_id,
        project_id,
        environment_id,
        profile_id,
        ConnectorRevisionId::new(),
        ConnectorDefinition::Http(ConnectorHttpDefinition::from_spec(
            ConnectorHttpDefinitionSpec {
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
            },
        )?),
        actor,
        created_at,
    )?;
    let profile = ConnectorProfile::create(
        profile_id,
        ResourceName::parse("Notification delivery")?,
        &revision,
    )?;
    let record = ConnectorRecord::new(profile, revision.clone())?;
    let request_id = Uuid::now_v7();
    repository
        .create(CreateConnectorProfileWrite {
            event: ConnectorRevisionPublished::created(
                &record.profile,
                &record.revision,
                request_id,
            )?,
            actor_principal_id: actor,
            request_id,
            idempotency: IdempotencyRequest::new(
                "notification-outbound-connector",
                "create",
                revision.definition.digest().as_str().as_bytes(),
            )?,
            record,
        })
        .await?;
    Ok(revision)
}

async fn create_outbound_subscription(
    repository: &PostgresNotificationRepository,
    organization_id: OrganizationId,
    recipient: PrincipalId,
    target: OutboundNotificationConnectorTarget,
    created_at: chrono::DateTime<Utc>,
) -> Result<OutboundNotificationSubscription, Box<dyn std::error::Error>> {
    let definition = OutboundNotificationSubscriptionDefinition::from_spec_with_suppression(
        OutboundNotificationSubscriptionSpec {
            channel: OutboundNotificationChannel::SlackCompatible,
            minimum_severity: NotificationSeverity::Information,
            target,
        },
        2,
        created_at + ChronoDuration::seconds(1),
    )?;
    let subscription = OutboundNotificationSubscription::create(
        organization_id,
        NotificationSubscriptionId::new(),
        recipient,
        definition.clone(),
        recipient,
        created_at,
    )?;
    let request_id = Uuid::now_v7();
    let write = CreateOutboundNotificationSubscriptionWrite {
        event: OutboundNotificationSubscriptionEvent::envelope(
            "notification.outbound-subscription.created",
            &subscription,
            request_id,
        )?,
        actor_principal_id: recipient,
        request_id,
        idempotency: IdempotencyRequest::new(
            "notification-outbound-subscription",
            "create",
            definition.digest().as_str().as_bytes(),
        )?,
        subscription: subscription.clone(),
    };
    assert!(
        !repository
            .create_subscription(write.clone())
            .await?
            .replayed
    );
    assert!(repository.create_subscription(write).await?.replayed);
    Ok(subscription)
}

async fn source_notification(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    recipient: PrincipalId,
    title: &str,
    occurred_at: chrono::DateTime<Utc>,
) -> Result<Notification, Box<dyn std::error::Error>> {
    let event_id = Uuid::now_v7();
    let aggregate_id = Uuid::now_v7();
    let correlation_id = Uuid::now_v7();
    let notification = Notification::project(
        organization_id,
        recipient,
        event_id,
        "identity.membership.role-changed".into(),
        1,
        aggregate_id,
        1,
        correlation_id,
        NotificationSeverity::Information,
        title.into(),
        format!("{title}."),
        NotificationScope::Organization,
        occurred_at,
        occurred_at,
    )?;
    database
        .execute(
            sql_query::<()>("insert into outbox_events (event_id, event_key, schema_version, organization_id, aggregate_id, aggregate_version, occurred_at, correlation_id, causation_id, payload) values (")
                .bind(event_id)
                .append(", 'identity.membership.role-changed', 1, ")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(aggregate_id)
                .append(", 1, ")
                .bind(notification.occurred_at)
                .append(", ")
                .bind(correlation_id)
                .append(", null, ")
                .bind(json!({
                    "membership_id": aggregate_id,
                    "principal_id": recipient,
                    "role": "member"
                }))
                .append(")"),
        )
        .await?;
    Ok(notification)
}

async fn assert_suppressed_delivery_rejected(
    executor: &PostgresExecutor,
    delivery: &OutboundNotificationDelivery,
    subscription_id: NotificationSubscriptionId,
) -> Result<(), Box<dyn std::error::Error>> {
    let fact = delivery.requested_event()?;
    let payload_digest = Sha256Digest::from_bytes(&delivery.canonical_payload()?);
    let target = delivery.target();
    let organization_id = delivery.organization_id();
    let delivery_id = delivery.id();
    let notification_id = delivery.notification_id();
    let recipient_principal_id = delivery.recipient_principal_id();
    let maximum_provider_attempts = delivery.maximum_provider_attempts();
    let channel = delivery.channel();
    let occurred_at = delivery.occurred_at();
    let rejected = executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let database = NotificationTransaction::new(transaction);
                database
                    .execute(
                        sql_query::<()>("insert into outbox_events (event_id, event_key, schema_version, organization_id, aggregate_id, aggregate_version, occurred_at, correlation_id, causation_id, payload) values (")
                            .bind(fact.event_id)
                            .append(", ")
                            .bind(fact.event_key.as_str())
                            .append(", ")
                            .bind(fact.schema_version)
                            .append(", ")
                            .bind(fact.organization_id)
                            .append(", ")
                            .bind(fact.aggregate_id)
                            .append(", ")
                            .bind(fact.aggregate_version)
                            .append(", ")
                            .bind(fact.occurred_at)
                            .append(", ")
                            .bind(fact.correlation_id)
                            .append(", ")
                            .bind(fact.causation_id)
                            .append(", ")
                            .bind(fact.payload)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into notification_outbound_deliveries (organization_id, id, notification_id, recipient_principal_id, subscription_id, requested_event_id, payload_digest, maximum_provider_attempts, channel, connector_project_id, connector_environment_id, connector_profile_id, connector_revision_id, occurred_at, terminal_outcome, terminal_generation, terminal_attempt_id, terminal_at) values (")
                            .bind(organization_id.as_uuid())
                            .append(", ")
                            .bind(delivery_id)
                            .append(", ")
                            .bind(notification_id.as_uuid())
                            .append(", ")
                            .bind(recipient_principal_id.as_uuid())
                            .append(", ")
                            .bind(subscription_id.as_uuid())
                            .append(", ")
                            .bind(fact.event_id)
                            .append(", ")
                            .bind(payload_digest.as_str())
                            .append(", ")
                            .bind(maximum_provider_attempts)
                            .append(", ")
                            .bind(channel.as_str())
                            .append(", ")
                            .bind(target.project_id.as_uuid())
                            .append(", ")
                            .bind(target.environment_id.as_uuid())
                            .append(", ")
                            .bind(target.profile_id.as_uuid())
                            .append(", ")
                            .bind(target.revision_id.as_uuid())
                            .append(", ")
                            .bind(occurred_at)
                            .append(", null, null, null, null)"),
                    )
                    .await?;
                Ok::<(), DatabaseError<PostgresError>>(())
            })
        })
        .await;
    let error = rejected.expect_err(
        "database must reject a forged delivery authorization before the immutable event-time cutoff",
    );
    assert!(
        format!("{error:?}").contains(
            "Outbound notification delivery fact is not authorized by its exact inbox projection and versioned subscription policy"
        ),
        "database rejected the forged delivery for an unexpected reason: {error:?}"
    );
    Ok(())
}

async fn assert_null_suppression_rejected(
    database: &Database<PostgresDialect, PostgresExecutor>,
    subscription: &OutboundNotificationSubscription,
) -> Result<(), Box<dyn std::error::Error>> {
    let spec = subscription.definition.spec();
    let rejected = database
        .execute(
            sql_query::<()>("insert into notification_outbound_subscriptions (organization_id, id, recipient_principal_id, channel, minimum_severity, connector_project_id, connector_environment_id, connector_profile_id, connector_revision_id, definition_schema, maximum_provider_attempts, suppress_before, canonical_acl, definition_digest, aggregate_version, created_by, created_at, revoked_at) values (")
                .bind(subscription.organization_id.as_uuid())
                .append(", ")
                .bind(NotificationSubscriptionId::new().as_uuid())
                .append(", ")
                .bind(subscription.recipient_principal_id.as_uuid())
                .append(", ")
                .bind(spec.channel.as_str())
                .append(", ")
                .bind(spec.minimum_severity.as_str())
                .append(", ")
                .bind(spec.target.project_id.as_uuid())
                .append(", ")
                .bind(spec.target.environment_id.as_uuid())
                .append(", ")
                .bind(spec.target.profile_id.as_uuid())
                .append(", ")
                .bind(spec.target.revision_id.as_uuid())
                .append(", ")
                .bind(subscription.definition.definition_schema())
                .append(", ")
                .bind(subscription.definition.maximum_provider_attempts())
                .append(", null, ")
                .bind(subscription.definition.canonical_acl())
                .append(", ")
                .bind(subscription.definition.digest().as_str())
                .append(", 2, ")
                .bind(subscription.created_by.as_uuid())
                .append(", ")
                .bind(subscription.created_at)
                .append(", ")
                .bind(subscription.created_at)
                .append(")"),
        )
        .await;
    let error = rejected.expect_err("database must reject a v3 subscription without a cutoff");
    assert!(
        format!("{error:?}")
            .contains("notification_outbound_subscriptions_definition_policy_check"),
        "database rejected a null v3 cutoff for an unexpected reason: {error:?}"
    );
    Ok(())
}

struct NotificationTransaction<'a> {
    transaction: &'a PostgresTransaction,
}

impl<'a> NotificationTransaction<'a> {
    const fn new(transaction: &'a PostgresTransaction) -> Self {
        Self { transaction }
    }

    async fn execute<Q>(&self, query: Q) -> Result<(), DatabaseError<PostgresError>>
    where
        Q: Query,
    {
        let query = query
            .compile(&PostgresDialect)
            .map_err(DatabaseError::Build)?;
        self.transaction
            .execute(&query)
            .await
            .map_err(DatabaseError::Execute)?;
        Ok(())
    }
}

fn assert_rejected<T, E: std::fmt::Debug>(result: Result<T, E>, label: &str) {
    assert!(result.is_err(), "database must reject {label}");
}
