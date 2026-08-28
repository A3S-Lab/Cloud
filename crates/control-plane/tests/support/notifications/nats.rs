use super::*;

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
pub(super) async fn exercise_notification_nats_delivery(
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
