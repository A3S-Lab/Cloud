use super::*;

pub(super) async fn seed_verified_contact(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
    membership_id: Uuid,
    contact_id: RecipientContactId,
    created_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'SMTP notification tenant', ")
            .bind(format!("smtp-notification-{organization_id}"))
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (",
            )
            .bind(principal_id.as_uuid())
            .append(", 'human', 'SMTP notification recipient', 1, ")
            .bind(created_at)
            .append(", null)"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into organization_memberships (id, organization_id, principal_id, role, aggregate_version, created_at, updated_at, revoked_at) values (",
            )
            .bind(membership_id)
            .append(", ")
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(principal_id.as_uuid())
            .append(", 'member', 1, ")
            .bind(created_at)
            .append(", ")
            .bind(created_at)
            .append(", null)"),
        )
        .await?;
    let address = "smtp-recipient@example.test";
    database
        .execute(
            sql_query::<()>(
                "insert into recipient_contacts (id, principal_id, canonical_address, address_digest, aggregate_version, state, created_at, updated_at, verified_at, revoked_at) values (",
            )
            .bind(contact_id.as_uuid())
            .append(", ")
            .bind(principal_id.as_uuid())
            .append(", ")
            .bind(address)
            .append(", ")
            .bind(Sha256Digest::from_bytes(address.as_bytes()).as_str())
            .append(", 2, 'verified', ")
            .bind(created_at)
            .append(", ")
            .bind(created_at)
            .append(", ")
            .bind(created_at)
            .append(", null)"),
        )
        .await?;
    Ok(())
}

pub(super) async fn project_delivery(
    database: &Database<PostgresDialect, PostgresExecutor>,
    repository: &PostgresNotificationRepository,
    subscription: &OutboundNotificationSubscription,
    title: &str,
    occurred_at: chrono::DateTime<Utc>,
) -> Result<OutboundNotificationDelivery, Box<dyn std::error::Error>> {
    let event_id = Uuid::now_v7();
    let aggregate_id = Uuid::now_v7();
    let correlation_id = Uuid::now_v7();
    let notification = Notification::project(
        subscription.organization_id,
        subscription.recipient_principal_id,
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
            sql_query::<()>(
                "insert into outbox_events (event_id, event_key, schema_version, organization_id, aggregate_id, aggregate_version, occurred_at, correlation_id, causation_id, payload) values (",
            )
            .bind(event_id)
            .append(", 'identity.membership.role-changed', 1, ")
            .bind(subscription.organization_id.as_uuid())
            .append(", ")
            .bind(aggregate_id)
            .append(", 1, ")
            .bind(notification.occurred_at)
            .append(", ")
            .bind(correlation_id)
            .append(", null, ")
            .bind(json!({
                "membership_id": aggregate_id,
                "principal_id": subscription.recipient_principal_id,
                "role": "member"
            }))
            .append(")"),
        )
        .await?;
    assert!(repository.project(notification.clone()).await?);
    let delivery = subscription.definition.delivery_for(&notification)?;
    assert!(matches!(
        repository.admit_delivery(&delivery).await?,
        Some(OutboundNotificationDeliveryAdmission::Pending)
    ));
    Ok(delivery)
}

pub(super) async fn reserve_and_start(
    repository: &PostgresNotificationRepository,
    delivery: &OutboundNotificationDelivery,
    generation: u64,
    fence_token: Uuid,
    reserved_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    assert!(matches!(
        repository
            .reserve_smtp_attempt(
                delivery,
                generation,
                fence_token,
                reserved_at,
                reserved_at + ChronoDuration::seconds(60),
            )
            .await?,
        OutboundNotificationSmtpAttemptAdmission::Reserved(_)
    ));
    assert!(matches!(
        repository
            .start_smtp_dispatch(
                delivery,
                generation,
                fence_token,
                reserved_at + ChronoDuration::seconds(1),
                reserved_at + ChronoDuration::seconds(11),
            )
            .await?,
        OutboundNotificationSmtpDispatchStart::Authorized(_)
    ));
    Ok(())
}

pub(super) async fn set_mailpit_chaos(
    client: &reqwest::Client,
    mailpit_api: &str,
    sender_error_code: Option<u16>,
) -> Result<(), Box<dyn std::error::Error>> {
    let body = sender_error_code.map_or_else(
        || json!({}),
        |error_code| {
            json!({
                "Sender": {
                    "ErrorCode": error_code,
                    "Probability": 100
                }
            })
        },
    );
    client
        .put(format!(
            "{}/api/v1/chaos",
            mailpit_api.trim_end_matches('/')
        ))
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

pub(super) async fn wait_for_terminal_receipt(
    repository: &PostgresNotificationRepository,
    delivery: &OutboundNotificationDelivery,
    timeout_seconds: u64,
) -> Result<OutboundNotificationTerminalReceipt, Box<dyn std::error::Error>> {
    let receipt = tokio::time::timeout(Duration::from_secs(timeout_seconds), async {
        loop {
            match repository.admit_delivery(delivery).await {
                Ok(Some(OutboundNotificationDeliveryAdmission::Terminal(receipt))) => {
                    break Ok::<_, RepositoryError>(receipt);
                }
                Ok(_) => tokio::time::sleep(Duration::from_millis(20)).await,
                Err(error) => break Err(error),
            }
        }
    })
    .await??;
    Ok(receipt)
}

pub(super) async fn wait_for_smtp_attempt_outcome(
    repository: &PostgresNotificationRepository,
    delivery: &OutboundNotificationDelivery,
    generation: u64,
    expected: OutboundNotificationSmtpAttemptOutcome,
    timeout_seconds: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(Duration::from_secs(timeout_seconds), async {
        loop {
            match repository
                .find_smtp_attempt(delivery.organization_id(), delivery.id(), generation)
                .await
            {
                Ok(Some(attempt)) if attempt.outcome == Some(expected) => {
                    break Ok::<_, RepositoryError>(());
                }
                Ok(_) => tokio::time::sleep(Duration::from_millis(20)).await,
                Err(error) => break Err(error),
            }
        }
    })
    .await??;
    Ok(())
}

pub(super) type SmtpNotificationConsumerTask = tokio::task::JoinHandle<a3s_event::Result<()>>;

pub(super) fn start_smtp_notification_consumer(
    bus: Arc<a3s_event::EventBus>,
    subject: String,
    deliveries: Arc<dyn IOutboundNotificationDeliveryRepository>,
    dispatcher: Arc<dyn IOutboundNotificationDispatcher>,
) -> Result<(watch::Sender<bool>, SmtpNotificationConsumerTask), Box<dyn std::error::Error>> {
    let consumer = A3sEventOutboundNotificationConsumer::new(bus, subject, deliveries, dispatcher)?;
    let (shutdown, receiver) = watch::channel(false);
    Ok((shutdown, tokio::spawn(consumer.run(receiver))))
}

pub(super) async fn stop_smtp_notification_consumer(
    shutdown: watch::Sender<bool>,
    task: SmtpNotificationConsumerTask,
) -> Result<(), Box<dyn std::error::Error>> {
    shutdown.send(true)?;
    tokio::time::timeout(Duration::from_secs(5), task).await???;
    Ok(())
}

pub(super) fn smtp_delivery_event(
    delivery: &OutboundNotificationDelivery,
    subject: &str,
) -> Result<Event, String> {
    let fact = delivery.requested_event()?;
    let mut event = Event::typed(
        subject,
        "cloud",
        OUTBOUND_NOTIFICATION_EVENT_KEY,
        fact.schema_version,
        OUTBOUND_NOTIFICATION_EVENT_KEY,
        "a3s-cloud",
        json!({
            "organizationId": fact.organization_id(),
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

pub(super) fn required_environment(name: &str) -> Result<String, std::io::Error> {
    std::env::var(name)
        .map_err(|_| std::io::Error::other(format!("required test environment {name} is not set")))
}
