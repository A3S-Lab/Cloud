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
use a3s_cloud_control_plane::modules::notifications::{
    outbound_notification_attempt_id, CreateOutboundNotificationSubscriptionWrite,
    INotificationRepository, IOutboundNotificationDeliveryRepository,
    IOutboundNotificationRepository, MarkNotificationReadWrite, Notification, NotificationScope,
    NotificationSeverity, OutboundNotificationChannel, OutboundNotificationConnectorTarget,
    OutboundNotificationDelivery, OutboundNotificationDeliveryAdmission,
    OutboundNotificationSubscription, OutboundNotificationSubscriptionDefinition,
    OutboundNotificationSubscriptionEvent, OutboundNotificationSubscriptionSpec,
    OutboundNotificationTerminalReceipt, PostgresNotificationRepository,
    MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotencyRequest,
    NotificationSubscriptionId, OrganizationId, PrincipalId, ProjectId, ResourceName,
};
use chrono::Duration as ChronoDuration;

pub(super) async fn exercise_notification_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = connect_and_migrate(&url, 4).await?;
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

    let first = source_notification(
        &database,
        organization_id,
        recipient,
        "Organization access granted",
        created_at + ChronoDuration::seconds(1),
    )
    .await?;
    let repository = notification_repository;
    assert!(repository.project(first.clone()).await?);
    assert!(!repository.project(first.clone()).await?);
    let outbound_delivery = OutboundNotificationDelivery::from_notification(
        &first,
        subscription.definition.spec().channel,
        target,
    )?;
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

    let exhausted_delivery = OutboundNotificationDelivery::from_notification(
        &concurrent,
        subscription.definition.spec().channel,
        target,
    )?;
    let exhausted_attempt_id = outbound_notification_attempt_id(
        exhausted_delivery.id(),
        MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
    )?;
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
        MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
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
        2
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
        1
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
    assert_eq!(evidence, (2, 1, 1, 1));
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
    Ok(())
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
    let definition = OutboundNotificationSubscriptionDefinition::from_spec(
        OutboundNotificationSubscriptionSpec {
            channel: OutboundNotificationChannel::SlackCompatible,
            minimum_severity: NotificationSeverity::Information,
            target,
        },
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

fn assert_rejected<T, E: std::fmt::Debug>(result: Result<T, E>, label: &str) {
    assert!(result.is_err(), "database must reject {label}");
}
