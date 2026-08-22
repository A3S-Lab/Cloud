use super::*;
use a3s_cloud_control_plane::modules::identity::domain::value_objects::RecipientEmailAddress;
use a3s_cloud_control_plane::modules::identity::PostgresIdentityRepository;
use a3s_cloud_control_plane::modules::notifications::{
    A3sEventOutboundNotificationConsumer, CreateOutboundNotificationSubscriptionWrite,
    INotificationRepository, IOutboundNotificationDeliveryRepository,
    IOutboundNotificationDispatcher, IOutboundNotificationRepository,
    IOutboundNotificationSmtpAttemptRepository, IOutboundNotificationSmtpDeliveryService,
    IPreparedOutboundNotificationSmtpDelivery, Notification, NotificationScope,
    NotificationSeverity, OutboundNotificationDelivery, OutboundNotificationDeliveryAdmission,
    OutboundNotificationDispatchResult, OutboundNotificationSmtpAttemptAdmission,
    OutboundNotificationSmtpAttemptOutcome, OutboundNotificationSmtpAttemptState,
    OutboundNotificationSmtpDispatchStart, OutboundNotificationSmtpDispatcher,
    OutboundNotificationSmtpPreparationError, OutboundNotificationSmtpProviderOutcome,
    OutboundNotificationSubscription, OutboundNotificationSubscriptionDefinition,
    OutboundNotificationSubscriptionEvent, OutboundNotificationTerminalOutcome,
    OutboundNotificationTerminalReceipt, PostgresNotificationRepository,
    SmtpOutboundNotificationCredentials, SmtpOutboundNotificationDeliveryOptions,
    SmtpOutboundNotificationDeliveryService, SmtpOutboundNotificationTlsPolicy,
    OUTBOUND_NOTIFICATION_EVENT_KEY,
};
use a3s_cloud_control_plane::modules::shared_kernel::application::ApplicationResult;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    IdempotencyRequest, NotificationSubscriptionId, OrganizationId, PrincipalId,
    RecipientContactId, RepositoryError, Sha256Digest,
};
use a3s_event::Event;
use chrono::Duration as ChronoDuration;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Notify};
use zeroize::Zeroizing;

const AMBIGUOUS_SMTP_TITLE: &str = "Ambiguous SMTP delivery";

struct CountingDeliveryRepository {
    inner: PostgresNotificationRepository,
    admissions: AtomicUsize,
    generic_settlements: AtomicUsize,
    admission_changed: Notify,
}

impl CountingDeliveryRepository {
    fn new(inner: PostgresNotificationRepository) -> Self {
        Self {
            inner,
            admissions: AtomicUsize::new(0),
            generic_settlements: AtomicUsize::new(0),
            admission_changed: Notify::new(),
        }
    }

    fn admission_count(&self) -> usize {
        self.admissions.load(Ordering::SeqCst)
    }

    fn generic_settlement_count(&self) -> usize {
        self.generic_settlements.load(Ordering::SeqCst)
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
        self.generic_settlements.fetch_add(1, Ordering::SeqCst);
        self.inner.settle_delivery(delivery, receipt).await
    }
}

struct ObservedSmtpDeliveryService {
    inner: Arc<dyn IOutboundNotificationSmtpDeliveryService>,
    provider_calls: Arc<AtomicUsize>,
}

impl ObservedSmtpDeliveryService {
    fn new(inner: Arc<dyn IOutboundNotificationSmtpDeliveryService>) -> Self {
        Self {
            inner,
            provider_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn provider_call_count(&self) -> usize {
        self.provider_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl IOutboundNotificationSmtpDeliveryService for ObservedSmtpDeliveryService {
    async fn prepare(
        &self,
        delivery: &OutboundNotificationDelivery,
        address: RecipientEmailAddress,
    ) -> Result<
        Box<dyn IPreparedOutboundNotificationSmtpDelivery>,
        OutboundNotificationSmtpPreparationError,
    > {
        let force_indeterminate = delivery.title() == AMBIGUOUS_SMTP_TITLE;
        let inner = self.inner.prepare(delivery, address).await?;
        Ok(Box::new(ObservedPreparedSmtpDelivery {
            inner,
            provider_calls: Arc::clone(&self.provider_calls),
            force_indeterminate,
        }))
    }
}

struct ObservedPreparedSmtpDelivery {
    inner: Box<dyn IPreparedOutboundNotificationSmtpDelivery>,
    provider_calls: Arc<AtomicUsize>,
    force_indeterminate: bool,
}

#[async_trait]
impl IPreparedOutboundNotificationSmtpDelivery for ObservedPreparedSmtpDelivery {
    async fn deliver(self: Box<Self>) -> OutboundNotificationSmtpProviderOutcome {
        let Self {
            inner,
            provider_calls,
            force_indeterminate,
        } = *self;
        provider_calls.fetch_add(1, Ordering::SeqCst);
        let outcome = inner.deliver().await;
        if force_indeterminate && outcome == OutboundNotificationSmtpProviderOutcome::Accepted {
            // Simulate an accepted DATA transaction whose final provider result was lost
            // after the Notifications dispatch fence crossed the provider boundary.
            OutboundNotificationSmtpProviderOutcome::Indeterminate
        } else {
            outcome
        }
    }
}

struct SmtpOnlyDispatcher {
    inner: Arc<OutboundNotificationSmtpDispatcher>,
}

#[async_trait]
impl IOutboundNotificationDispatcher for SmtpOnlyDispatcher {
    async fn dispatch(
        &self,
        delivery: &OutboundNotificationDelivery,
        delivery_count: u64,
    ) -> ApplicationResult<OutboundNotificationDispatchResult> {
        self.inner.dispatch(delivery, delivery_count).await
    }
}

pub(super) async fn exercise_outbound_smtp_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 8).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let repository = PostgresNotificationRepository::new(executor);
    let organization_id = OrganizationId::new();
    let principal_id = PrincipalId::new();
    let membership_id = Uuid::now_v7();
    let contact_id = RecipientContactId::new();
    let identity_created_at = Utc::now() - ChronoDuration::minutes(1);
    seed_verified_contact(
        &database,
        organization_id,
        principal_id,
        membership_id,
        contact_id,
        identity_created_at,
    )
    .await?;

    let definition = OutboundNotificationSubscriptionDefinition::from_smtp_spec(
        contact_id,
        NotificationSeverity::Information,
        2,
        None,
    )?;
    let subscription = OutboundNotificationSubscription::create(
        organization_id,
        NotificationSubscriptionId::new(),
        principal_id,
        definition.clone(),
        principal_id,
        identity_created_at + ChronoDuration::seconds(1),
    )?;
    let request_id = Uuid::now_v7();
    let subscription_write = CreateOutboundNotificationSubscriptionWrite {
        event: OutboundNotificationSubscriptionEvent::envelope(
            "notification.outbound-subscription.created",
            &subscription,
            request_id,
        )?,
        subscription: subscription.clone(),
        actor_principal_id: principal_id,
        request_id,
        idempotency: IdempotencyRequest::new(
            "tests/outbound-smtp",
            "create-subscription",
            definition.digest().as_str().as_bytes(),
        )?,
    };
    assert!(
        !repository
            .create_subscription(subscription_write.clone())
            .await?
            .replayed
    );
    assert!(
        repository
            .create_subscription(subscription_write)
            .await?
            .replayed
    );

    let accepted = project_delivery(
        &database,
        &repository,
        &subscription,
        "Accepted SMTP delivery",
        subscription.created_at + ChronoDuration::seconds(1),
    )
    .await?;
    let accepted_token = Uuid::now_v7();
    let accepted_reserved_at = accepted.occurred_at() + ChronoDuration::seconds(1);
    let accepted_reservation = repository
        .reserve_smtp_attempt(
            &accepted,
            1,
            accepted_token,
            accepted_reserved_at,
            accepted_reserved_at + ChronoDuration::seconds(60),
        )
        .await?;
    assert!(matches!(
        accepted_reservation,
        OutboundNotificationSmtpAttemptAdmission::Reserved(ref attempt)
            if attempt.fence_generation == 1 && attempt.fence_token == accepted_token
    ));
    assert!(matches!(
        repository
            .start_smtp_dispatch(
                &accepted,
                1,
                accepted_token,
                accepted_reserved_at + ChronoDuration::seconds(1),
                accepted_reserved_at + ChronoDuration::seconds(11),
            )
            .await?,
        OutboundNotificationSmtpDispatchStart::Authorized(ref attempt)
            if attempt.state == OutboundNotificationSmtpAttemptState::Dispatching
    ));
    let accepted_settlement = repository
        .settle_smtp_attempt(
            &accepted,
            1,
            accepted_token,
            OutboundNotificationSmtpAttemptOutcome::Accepted,
            accepted_reserved_at + ChronoDuration::seconds(2),
        )
        .await?;
    let accepted_receipt = accepted_settlement
        .receipt
        .clone()
        .ok_or("accepted SMTP settlement omitted its receipt")?;
    assert_eq!(
        accepted_receipt.outcome(),
        OutboundNotificationTerminalOutcome::Delivered
    );
    assert_eq!(
        repository.admit_delivery(&accepted).await?,
        Some(OutboundNotificationDeliveryAdmission::Terminal(
            accepted_receipt.clone()
        ))
    );
    assert!(matches!(
        repository
            .reserve_smtp_attempt(
                &accepted,
                1,
                Uuid::now_v7(),
                accepted_reserved_at + ChronoDuration::seconds(3),
                accepted_reserved_at + ChronoDuration::seconds(63),
            )
            .await?,
        OutboundNotificationSmtpAttemptAdmission::Terminal(receipt)
            if receipt == accepted_receipt
    ));
    assert!(repository
        .find_smtp_attempt(OrganizationId::new(), accepted.id(), 1)
        .await?
        .is_none());
    assert!(database
        .execute(
            sql_query::<()>(
                "update notification_outbound_smtp_attempts set outcome = 'rejected' where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and delivery_id = ")
            .bind(accepted.id())
            .append(" and generation = 1"),
        )
        .await
        .is_err());

    let rejected = project_delivery(
        &database,
        &repository,
        &subscription,
        "Rejected SMTP delivery",
        accepted.occurred_at() + ChronoDuration::seconds(1),
    )
    .await?;
    let rejected_token = Uuid::now_v7();
    let rejected_at = rejected.occurred_at() + ChronoDuration::seconds(1);
    reserve_and_start(&repository, &rejected, 1, rejected_token, rejected_at).await?;
    let rejected_receipt = repository
        .settle_smtp_attempt(
            &rejected,
            1,
            rejected_token,
            OutboundNotificationSmtpAttemptOutcome::Rejected,
            rejected_at + ChronoDuration::seconds(2),
        )
        .await?
        .receipt
        .ok_or("rejected SMTP settlement omitted its receipt")?;
    assert_eq!(
        rejected_receipt.outcome(),
        OutboundNotificationTerminalOutcome::Rejected
    );
    assert_eq!(
        repository.admit_delivery(&rejected).await?,
        Some(OutboundNotificationDeliveryAdmission::Terminal(
            rejected_receipt
        ))
    );

    let retryable = project_delivery(
        &database,
        &repository,
        &subscription,
        "Retryable SMTP delivery",
        rejected.occurred_at() + ChronoDuration::seconds(1),
    )
    .await?;
    let first_retry_token = Uuid::now_v7();
    let first_retry_at = retryable.occurred_at() + ChronoDuration::seconds(1);
    reserve_and_start(
        &repository,
        &retryable,
        1,
        first_retry_token,
        first_retry_at,
    )
    .await?;
    let first_retry = repository
        .settle_smtp_attempt(
            &retryable,
            1,
            first_retry_token,
            OutboundNotificationSmtpAttemptOutcome::Retryable,
            first_retry_at + ChronoDuration::seconds(2),
        )
        .await?;
    assert!(first_retry.receipt.is_none());
    assert!(matches!(
        repository
            .reserve_smtp_attempt(
                &retryable,
                1,
                Uuid::now_v7(),
                first_retry_at + ChronoDuration::seconds(3),
                first_retry_at + ChronoDuration::seconds(63),
            )
            .await?,
        OutboundNotificationSmtpAttemptAdmission::Retryable(_)
    ));
    let final_retry_token = Uuid::now_v7();
    let final_retry_at = first_retry_at + ChronoDuration::seconds(4);
    reserve_and_start(
        &repository,
        &retryable,
        2,
        final_retry_token,
        final_retry_at,
    )
    .await?;
    let exhausted = repository
        .settle_smtp_attempt(
            &retryable,
            2,
            final_retry_token,
            OutboundNotificationSmtpAttemptOutcome::Retryable,
            final_retry_at + ChronoDuration::seconds(2),
        )
        .await?
        .receipt
        .ok_or("final retryable SMTP settlement omitted Exhausted receipt")?;
    assert_eq!(
        exhausted.outcome(),
        OutboundNotificationTerminalOutcome::Exhausted
    );
    assert!(matches!(
        repository
            .reserve_smtp_attempt(
                &retryable,
                3,
                Uuid::now_v7(),
                final_retry_at + ChronoDuration::seconds(3),
                final_retry_at + ChronoDuration::seconds(63),
            )
            .await?,
        OutboundNotificationSmtpAttemptAdmission::InvalidFact
    ));

    let indeterminate = project_delivery(
        &database,
        &repository,
        &subscription,
        "Indeterminate SMTP delivery",
        retryable.occurred_at() + ChronoDuration::seconds(1),
    )
    .await?;
    let indeterminate_token = Uuid::now_v7();
    let indeterminate_at = indeterminate.occurred_at() + ChronoDuration::seconds(1);
    reserve_and_start(
        &repository,
        &indeterminate,
        1,
        indeterminate_token,
        indeterminate_at,
    )
    .await?;
    let outcome_deadline = indeterminate_at + ChronoDuration::seconds(11);
    let forged_receipt = OutboundNotificationTerminalReceipt::from_smtp_outcome(
        &indeterminate,
        1,
        OutboundNotificationSmtpAttemptOutcome::Indeterminate,
        outcome_deadline,
    )?
    .ok_or("indeterminate SMTP outcome omitted its receipt")?;
    assert!(repository
        .settle_delivery(&indeterminate, forged_receipt)
        .await
        .is_err());
    assert!(matches!(
        repository.admit_delivery(&indeterminate).await?,
        Some(OutboundNotificationDeliveryAdmission::Pending)
    ));
    let recovered = repository
        .reserve_smtp_attempt(
            &indeterminate,
            1,
            Uuid::now_v7(),
            outcome_deadline,
            outcome_deadline + ChronoDuration::seconds(60),
        )
        .await?;
    assert!(matches!(
        recovered,
        OutboundNotificationSmtpAttemptAdmission::Terminal(ref receipt)
            if receipt.outcome() == OutboundNotificationTerminalOutcome::Indeterminate
                && receipt.terminal_at() == outcome_deadline
    ));

    let obsolete = project_delivery(
        &database,
        &repository,
        &subscription,
        "Obsolete SMTP delivery",
        indeterminate.occurred_at() + ChronoDuration::seconds(1),
    )
    .await?;
    let authority_revoked_at = obsolete.occurred_at() + ChronoDuration::seconds(1);
    assert_eq!(
        database
            .execute(
                sql_query::<()>(
                    "update organization_memberships set aggregate_version = aggregate_version + 1, updated_at = ",
                )
                .bind(authority_revoked_at)
                .append(", revoked_at = ")
                .bind(authority_revoked_at)
                .append(" where id = ")
                .bind(membership_id),
            )
            .await?
            .rows_affected,
        1
    );
    let obsolete_receipt = match repository
        .reserve_smtp_attempt(
            &obsolete,
            1,
            Uuid::now_v7(),
            authority_revoked_at + ChronoDuration::seconds(1),
            authority_revoked_at + ChronoDuration::seconds(61),
        )
        .await?
    {
        OutboundNotificationSmtpAttemptAdmission::Terminal(receipt) => receipt,
        other => {
            return Err(format!("authority loss did not obsolete SMTP delivery: {other:?}").into())
        }
    };
    assert_eq!(
        obsolete_receipt.outcome(),
        OutboundNotificationTerminalOutcome::Obsolete
    );
    let obsolete_attempt = repository
        .find_smtp_attempt(organization_id, obsolete.id(), 1)
        .await?
        .ok_or("obsolete SMTP attempt was not persisted")?;
    assert_eq!(
        obsolete_attempt.state,
        OutboundNotificationSmtpAttemptState::Terminal
    );
    assert_eq!(
        obsolete_attempt.outcome,
        Some(OutboundNotificationSmtpAttemptOutcome::Obsolete)
    );
    assert!(obsolete_attempt.dispatch_started_at.is_none());

    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from notification_outbound_deliveries where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and terminal_outcome is not null"),
            )
            .await?,
        5
    );
    println!(
        "A3S_CLOUD_C0_3_N5E_POSTGRES_CERTIFIED migration=138 accepted=1 rejected=1 retryable=1 exhausted=1 indeterminate=1 obsolete=1 atomic_receipts=5"
    );
    Ok(())
}

pub(super) async fn exercise_outbound_smtp_provider_delivery(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let nats_url = required_environment("A3S_CLOUD_TEST_NATS_URL")?;
    let mailpit_api = required_environment("A3S_CLOUD_TEST_MAILPIT_API")?;
    let smtp_host = required_environment("A3S_CLOUD_TEST_SMTP_HOST")?;
    let smtp_port = required_environment("A3S_CLOUD_TEST_SMTP_PORT")?.parse::<u16>()?;
    let smtp_ca_file = required_environment("A3S_CLOUD_TEST_SMTP_CA_FILE")?;
    let smtp_username = Zeroizing::new(required_environment("A3S_CLOUD_TEST_SMTP_USERNAME")?);
    let smtp_password = Zeroizing::new(required_environment("A3S_CLOUD_TEST_SMTP_PASSWORD")?);

    let executor = migrate_and_connect_for_test(&url, 8).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let repository = Arc::new(PostgresNotificationRepository::new(executor.clone()));
    let organization_id = OrganizationId::new();
    let principal_id = PrincipalId::new();
    let membership_id = Uuid::now_v7();
    let contact_id = RecipientContactId::new();
    let identity_created_at = Utc::now() - ChronoDuration::minutes(1);
    seed_verified_contact(
        &database,
        organization_id,
        principal_id,
        membership_id,
        contact_id,
        identity_created_at,
    )
    .await?;

    let definition = OutboundNotificationSubscriptionDefinition::from_smtp_spec(
        contact_id,
        NotificationSeverity::Information,
        2,
        None,
    )?;
    let subscription = OutboundNotificationSubscription::create(
        organization_id,
        NotificationSubscriptionId::new(),
        principal_id,
        definition.clone(),
        principal_id,
        identity_created_at + ChronoDuration::seconds(1),
    )?;
    let subscription_request_id = Uuid::now_v7();
    repository
        .create_subscription(CreateOutboundNotificationSubscriptionWrite {
            event: OutboundNotificationSubscriptionEvent::envelope(
                "notification.outbound-subscription.created",
                &subscription,
                subscription_request_id,
            )?,
            subscription: subscription.clone(),
            actor_principal_id: principal_id,
            request_id: subscription_request_id,
            idempotency: IdempotencyRequest::new(
                "tests/outbound-smtp-provider",
                "create-subscription",
                definition.digest().as_str().as_bytes(),
            )?,
        })
        .await?;

    let production_delivery: Arc<dyn IOutboundNotificationSmtpDeliveryService> =
        Arc::new(SmtpOutboundNotificationDeliveryService::from_options(
            SmtpOutboundNotificationDeliveryOptions {
                host: smtp_host,
                port: smtp_port,
                tls_policy: SmtpOutboundNotificationTlsPolicy::RequiredStartTls,
                hello_name: "cloud.test.invalid".into(),
                ca_certificate_file: smtp_ca_file,
                sender: RecipientEmailAddress::parse("no-reply@example.test")?,
                credentials: SmtpOutboundNotificationCredentials {
                    username: smtp_username,
                    password: smtp_password,
                },
                connect_timeout: Duration::from_secs(5),
                command_timeout: Duration::from_secs(10),
            },
        )?);
    let observed_delivery = Arc::new(ObservedSmtpDeliveryService::new(production_delivery));
    let smtp_delivery: Arc<dyn IOutboundNotificationSmtpDeliveryService> =
        observed_delivery.clone();
    let attempts: Arc<dyn IOutboundNotificationSmtpAttemptRepository> = repository.clone();
    let recipient_contacts = Arc::new(PostgresIdentityRepository::new(executor.clone()));
    let smtp_dispatcher = Arc::new(OutboundNotificationSmtpDispatcher::new(
        attempts,
        recipient_contacts,
        smtp_delivery,
        ChronoDuration::seconds(60),
        ChronoDuration::seconds(10),
    )?);
    let dispatcher: Arc<dyn IOutboundNotificationDispatcher> = Arc::new(SmtpOnlyDispatcher {
        inner: smtp_dispatcher,
    });

    let nats_config = NatsConfig {
        url: nats_url,
        stream_name: format!("A3S_CLOUD_N5E_{}", Uuid::new_v4().simple()).to_uppercase(),
        subject_prefix: format!("a3s-cloud-n5e-{}", Uuid::new_v4().simple()).to_lowercase(),
        storage: StorageType::Memory,
        ..NatsConfig::default()
    };
    let publisher = Arc::new(A3sEventPublisher::nats(nats_config).await?);
    let bus = publisher.bus();
    let subject = publisher.subject(OUTBOUND_NOTIFICATION_EVENT_KEY);
    let counting_repository =
        Arc::new(CountingDeliveryRepository::new(repository.as_ref().clone()));
    let deliveries: Arc<dyn IOutboundNotificationDeliveryRepository> = counting_repository.clone();
    let (shutdown, consumer_task) = start_smtp_notification_consumer(
        Arc::clone(&bus),
        subject.clone(),
        deliveries,
        dispatcher,
    )?;
    let relay = OutboxRelay::new(
        Arc::new(PostgresOutboxRepository::new(executor.clone())),
        publisher,
        OutboxRelayConfig {
            batch_size: 1_000,
            poll_interval: Duration::from_millis(10),
            lease_duration: Duration::from_secs(5),
            publish_timeout: Duration::from_secs(2),
            initial_backoff: Duration::from_millis(10),
            maximum_backoff: Duration::from_millis(100),
        },
    )?;
    let mailpit = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .build()?;

    set_mailpit_chaos(&mailpit, &mailpit_api, None).await?;
    let accepted = project_delivery(
        &database,
        repository.as_ref(),
        &subscription,
        "Accepted SMTP provider delivery",
        subscription.created_at + ChronoDuration::seconds(1),
    )
    .await?;
    let report = relay.run_once().await?;
    assert!(report.claimed > 0 && report.failures.is_empty());
    let accepted_receipt = wait_for_terminal_receipt(repository.as_ref(), &accepted, 15).await?;
    assert_eq!(
        accepted_receipt.outcome(),
        OutboundNotificationTerminalOutcome::Delivered
    );
    assert_eq!(observed_delivery.provider_call_count(), 1);

    let accepted_admissions = counting_repository.admission_count();
    bus.publish_event(&smtp_delivery_event(&accepted, &subject)?)
        .await?;
    tokio::time::timeout(
        Duration::from_secs(10),
        counting_repository.wait_for_admission_after(accepted_admissions),
    )
    .await?;
    assert_eq!(observed_delivery.provider_call_count(), 1);

    set_mailpit_chaos(&mailpit, &mailpit_api, Some(451)).await?;
    let exhausted = project_delivery(
        &database,
        repository.as_ref(),
        &subscription,
        "Transient SMTP provider delivery",
        accepted.occurred_at() + ChronoDuration::seconds(1),
    )
    .await?;
    let report = relay.run_once().await?;
    assert!(report.claimed > 0 && report.failures.is_empty());
    wait_for_smtp_attempt_outcome(
        repository.as_ref(),
        &exhausted,
        1,
        OutboundNotificationSmtpAttemptOutcome::Retryable,
        15,
    )
    .await?;
    assert!(matches!(
        repository.admit_delivery(&exhausted).await?,
        Some(OutboundNotificationDeliveryAdmission::Pending)
    ));
    let exhausted_receipt = wait_for_terminal_receipt(repository.as_ref(), &exhausted, 50).await?;
    assert_eq!(
        exhausted_receipt.outcome(),
        OutboundNotificationTerminalOutcome::Exhausted
    );
    wait_for_smtp_attempt_outcome(
        repository.as_ref(),
        &exhausted,
        2,
        OutboundNotificationSmtpAttemptOutcome::Retryable,
        5,
    )
    .await?;
    assert_eq!(observed_delivery.provider_call_count(), 3);

    set_mailpit_chaos(&mailpit, &mailpit_api, Some(550)).await?;
    let rejected = project_delivery(
        &database,
        repository.as_ref(),
        &subscription,
        "Rejected SMTP provider delivery",
        exhausted.occurred_at() + ChronoDuration::seconds(1),
    )
    .await?;
    let report = relay.run_once().await?;
    assert!(report.claimed > 0 && report.failures.is_empty());
    let rejected_receipt = wait_for_terminal_receipt(repository.as_ref(), &rejected, 15).await?;
    assert_eq!(
        rejected_receipt.outcome(),
        OutboundNotificationTerminalOutcome::Rejected
    );
    assert_eq!(observed_delivery.provider_call_count(), 4);

    set_mailpit_chaos(&mailpit, &mailpit_api, None).await?;
    let indeterminate = project_delivery(
        &database,
        repository.as_ref(),
        &subscription,
        AMBIGUOUS_SMTP_TITLE,
        rejected.occurred_at() + ChronoDuration::seconds(1),
    )
    .await?;
    let report = relay.run_once().await?;
    assert!(report.claimed > 0 && report.failures.is_empty());
    let indeterminate_receipt =
        wait_for_terminal_receipt(repository.as_ref(), &indeterminate, 15).await?;
    assert_eq!(
        indeterminate_receipt.outcome(),
        OutboundNotificationTerminalOutcome::Indeterminate
    );
    assert_eq!(observed_delivery.provider_call_count(), 5);

    let indeterminate_admissions = counting_repository.admission_count();
    bus.publish_event(&smtp_delivery_event(&indeterminate, &subject)?)
        .await?;
    tokio::time::timeout(
        Duration::from_secs(10),
        counting_repository.wait_for_admission_after(indeterminate_admissions),
    )
    .await?;
    assert_eq!(observed_delivery.provider_call_count(), 5);

    let obsolete = project_delivery(
        &database,
        repository.as_ref(),
        &subscription,
        "Obsolete SMTP provider delivery",
        indeterminate.occurred_at() + ChronoDuration::seconds(1),
    )
    .await?;
    let revoked_at = Utc::now();
    assert_eq!(
        database
            .execute(
                sql_query::<()>(
                    "update organization_memberships set aggregate_version = aggregate_version + 1, updated_at = ",
                )
                .bind(revoked_at)
                .append(", revoked_at = ")
                .bind(revoked_at)
                .append(" where id = ")
                .bind(membership_id),
            )
            .await?
            .rows_affected,
        1
    );
    let report = relay.run_once().await?;
    assert!(report.claimed > 0 && report.failures.is_empty());
    let obsolete_receipt = wait_for_terminal_receipt(repository.as_ref(), &obsolete, 15).await?;
    assert_eq!(
        obsolete_receipt.outcome(),
        OutboundNotificationTerminalOutcome::Obsolete
    );
    assert_eq!(observed_delivery.provider_call_count(), 5);
    let obsolete_attempt = repository
        .find_smtp_attempt(organization_id, obsolete.id(), 1)
        .await?
        .ok_or("obsolete provider attempt is missing")?;
    assert!(obsolete_attempt.dispatch_started_at.is_none());

    let search = mailpit
        .get(format!(
            "{}/api/v1/search",
            mailpit_api.trim_end_matches('/')
        ))
        .query(&[("query", "to:smtp-recipient@example.test"), ("limit", "10")])
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?;
    assert_eq!(search["messages_count"], json!(2));
    let captured = mailpit
        .get(format!(
            "{}/view/latest.txt",
            mailpit_api.trim_end_matches('/')
        ))
        .query(&[("query", "to:smtp-recipient@example.test")])
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    assert!(captured.contains(AMBIGUOUS_SMTP_TITLE));

    set_mailpit_chaos(&mailpit, &mailpit_api, None).await?;
    stop_smtp_notification_consumer(shutdown, consumer_task).await?;
    assert_eq!(bus.info().await?.consumers, 1);
    assert_eq!(counting_repository.generic_settlement_count(), 0);
    println!(
        "A3S_CLOUD_C0_3_N5E_PROVIDER_CERTIFIED migration=138 jetstream=durable_manual_ack starttls=required auth=plain accepted=1 transient_attempts=2 rejected=1 indeterminate=1 obsolete=1 exhausted=1 ack_only_replays=2 provider_calls=5 mailpit_captured=2 generic_settlements=0"
    );
    Ok(())
}

#[path = "outbound_smtp/helpers.rs"]
mod helpers;

use helpers::*;
