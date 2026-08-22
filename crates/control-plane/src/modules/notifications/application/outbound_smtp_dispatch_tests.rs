use super::*;
use crate::modules::identity::domain::entities::{
    RecipientContactRecord, RecipientContactVerification,
};
use crate::modules::identity::domain::repositories::{
    BeginRecipientContactVerificationResult, BeginRecipientContactVerificationWrite,
    CompleteRecipientContactVerificationWrite, RevokeRecipientContactWrite,
};
use crate::modules::identity::domain::value_objects::RecipientEmailAddress;
use crate::modules::notifications::domain::{
    CreateOutboundNotificationSubscriptionWrite, INotificationRepository,
    IOutboundNotificationRepository, IPreparedOutboundNotificationSmtpDelivery, Notification,
    NotificationScope, NotificationSeverity, OutboundNotificationSmtpAttemptRecord,
    OutboundNotificationSubscription, OutboundNotificationSubscriptionDefinition,
    OutboundNotificationSubscriptionEvent, OutboundNotificationTerminalOutcome,
};
use crate::modules::notifications::InMemoryNotificationRepository;
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, NotificationSubscriptionId, OrganizationId, PrincipalId,
    RecipientContactId, RecipientContactVerificationId,
};
use async_trait::async_trait;
use std::collections::VecDeque;
use tokio::sync::Mutex;

type Resolution = Result<Option<ResolvedRecipientContact>, RepositoryError>;

struct ScriptedRecipientContacts {
    resolutions: Mutex<VecDeque<Resolution>>,
    phases: Arc<Mutex<Vec<&'static str>>>,
}

impl ScriptedRecipientContacts {
    fn new(resolutions: Vec<Resolution>, phases: Arc<Mutex<Vec<&'static str>>>) -> Self {
        Self {
            resolutions: Mutex::new(resolutions.into()),
            phases,
        }
    }
}

#[async_trait]
impl IRecipientContactRepository for ScriptedRecipientContacts {
    async fn begin_recipient_contact_verification(
        &self,
        _write: BeginRecipientContactVerificationWrite,
    ) -> Result<IdempotentWrite<BeginRecipientContactVerificationResult>, RepositoryError> {
        unexpected_contact_operation()
    }

    async fn find_recipient_contact(
        &self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
        _contact_id: RecipientContactId,
    ) -> Result<Option<RecipientContactRecord>, RepositoryError> {
        unexpected_contact_operation()
    }

    async fn list_recipient_contacts(
        &self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<Vec<RecipientContactRecord>, RepositoryError> {
        unexpected_contact_operation()
    }

    async fn find_recipient_contact_verification(
        &self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
        _contact_id: RecipientContactId,
        _verification_id: RecipientContactVerificationId,
    ) -> Result<Option<RecipientContactVerification>, RepositoryError> {
        unexpected_contact_operation()
    }

    async fn complete_recipient_contact_verification(
        &self,
        _write: CompleteRecipientContactVerificationWrite,
    ) -> Result<IdempotentWrite<RecipientContactRecord>, RepositoryError> {
        unexpected_contact_operation()
    }

    async fn revoke_recipient_contact(
        &self,
        _write: RevokeRecipientContactWrite,
    ) -> Result<IdempotentWrite<RecipientContactRecord>, RepositoryError> {
        unexpected_contact_operation()
    }

    async fn resolve_verified_recipient_contact(
        &self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
        _contact_id: RecipientContactId,
    ) -> Result<Option<ResolvedRecipientContact>, RepositoryError> {
        self.phases.lock().await.push("resolve");
        self.resolutions
            .lock()
            .await
            .pop_front()
            .unwrap_or_else(|| {
                Err(RepositoryError::Storage(
                    "recipient contact resolution script was exhausted".into(),
                ))
            })
    }
}

fn unexpected_contact_operation<T>() -> Result<T, RepositoryError> {
    Err(RepositoryError::Storage(
        "unexpected recipient contact repository operation".into(),
    ))
}

struct RecordingAttempts {
    repository: Arc<InMemoryNotificationRepository>,
    phases: Arc<Mutex<Vec<&'static str>>>,
}

#[async_trait]
impl IOutboundNotificationSmtpAttemptRepository for RecordingAttempts {
    async fn reserve_smtp_attempt(
        &self,
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        fence_token: Uuid,
        reserved_at: chrono::DateTime<Utc>,
        lease_expires_at: chrono::DateTime<Utc>,
    ) -> Result<OutboundNotificationSmtpAttemptAdmission, RepositoryError> {
        self.phases.lock().await.push("reserve");
        self.repository
            .reserve_smtp_attempt(
                delivery,
                generation,
                fence_token,
                reserved_at,
                lease_expires_at,
            )
            .await
    }

    async fn start_smtp_dispatch(
        &self,
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        fence_token: Uuid,
        started_at: chrono::DateTime<Utc>,
        outcome_deadline_at: chrono::DateTime<Utc>,
    ) -> Result<OutboundNotificationSmtpDispatchStart, RepositoryError> {
        self.phases.lock().await.push("start");
        self.repository
            .start_smtp_dispatch(
                delivery,
                generation,
                fence_token,
                started_at,
                outcome_deadline_at,
            )
            .await
    }

    async fn settle_smtp_attempt(
        &self,
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        fence_token: Uuid,
        outcome: OutboundNotificationSmtpAttemptOutcome,
        settled_at: chrono::DateTime<Utc>,
    ) -> Result<OutboundNotificationSmtpAttemptSettlement, RepositoryError> {
        self.phases.lock().await.push("settle");
        self.repository
            .settle_smtp_attempt(delivery, generation, fence_token, outcome, settled_at)
            .await
    }

    async fn find_smtp_attempt(
        &self,
        organization_id: OrganizationId,
        delivery_id: Uuid,
        generation: u64,
    ) -> Result<Option<OutboundNotificationSmtpAttemptRecord>, RepositoryError> {
        self.repository
            .find_smtp_attempt(organization_id, delivery_id, generation)
            .await
    }
}

struct ScriptedDeliveryService {
    preparations: Mutex<
        VecDeque<
            Result<
                OutboundNotificationSmtpProviderOutcome,
                OutboundNotificationSmtpPreparationError,
            >,
        >,
    >,
    phases: Arc<Mutex<Vec<&'static str>>>,
}

struct ScriptedPreparedDelivery {
    outcome: OutboundNotificationSmtpProviderOutcome,
    phases: Arc<Mutex<Vec<&'static str>>>,
}

#[async_trait]
impl IOutboundNotificationSmtpDeliveryService for ScriptedDeliveryService {
    async fn prepare(
        &self,
        _delivery: &OutboundNotificationDelivery,
        _address: RecipientEmailAddress,
    ) -> Result<
        Box<dyn IPreparedOutboundNotificationSmtpDelivery>,
        OutboundNotificationSmtpPreparationError,
    > {
        self.phases.lock().await.push("prepare");
        let outcome = self
            .preparations
            .lock()
            .await
            .pop_front()
            .unwrap_or(Err(OutboundNotificationSmtpPreparationError::Unavailable))?;
        Ok(Box::new(ScriptedPreparedDelivery {
            outcome,
            phases: Arc::clone(&self.phases),
        }))
    }
}

#[async_trait]
impl IPreparedOutboundNotificationSmtpDelivery for ScriptedPreparedDelivery {
    async fn deliver(self: Box<Self>) -> OutboundNotificationSmtpProviderOutcome {
        self.phases.lock().await.push("deliver");
        self.outcome
    }
}

async fn seeded_delivery(
    maximum_provider_attempts: u64,
) -> (
    Arc<InMemoryNotificationRepository>,
    OutboundNotificationDelivery,
) {
    let repository = Arc::new(InMemoryNotificationRepository::new());
    let organization_id = OrganizationId::new();
    let principal_id = PrincipalId::new();
    let contact_id = RecipientContactId::new();
    let occurred_at = canonical_timestamp(Utc::now() - Duration::seconds(1));
    let definition = OutboundNotificationSubscriptionDefinition::from_smtp_spec(
        contact_id,
        NotificationSeverity::Information,
        maximum_provider_attempts,
        None,
    )
    .expect("SMTP definition");
    let subscription = OutboundNotificationSubscription::create(
        organization_id,
        NotificationSubscriptionId::new(),
        principal_id,
        definition,
        principal_id,
        occurred_at,
    )
    .expect("SMTP subscription");
    let request_id = Uuid::now_v7();
    repository
        .create_subscription(CreateOutboundNotificationSubscriptionWrite {
            event: OutboundNotificationSubscriptionEvent::envelope(
                "notification.outbound-subscription.created",
                &subscription,
                request_id,
            )
            .expect("subscription event"),
            subscription,
            actor_principal_id: principal_id,
            request_id,
            idempotency: IdempotencyRequest::new(
                "test.notification.smtp-subscription",
                Uuid::now_v7().to_string(),
                b"smtp-subscription",
            )
            .expect("idempotency"),
        })
        .await
        .expect("store subscription");
    let notification = Notification::project(
        organization_id,
        principal_id,
        Uuid::now_v7(),
        "identity.membership.role-changed".into(),
        1,
        Uuid::now_v7(),
        1,
        Uuid::now_v7(),
        NotificationSeverity::Warning,
        "Organization role changed".into(),
        "Your organization role is now member.".into(),
        NotificationScope::Organization,
        occurred_at,
        occurred_at,
    )
    .expect("notification");
    repository
        .project(notification)
        .await
        .expect("project notification");
    let delivery = repository
        .outbound_deliveries()
        .await
        .into_iter()
        .next()
        .expect("outbound SMTP delivery");
    (repository, delivery)
}

fn resolved_contact(delivery: &OutboundNotificationDelivery) -> ResolvedRecipientContact {
    ResolvedRecipientContact {
        id: delivery.recipient_contact_id().expect("contact target"),
        principal_id: delivery.recipient_principal_id(),
        address: RecipientEmailAddress::parse("recipient@example.test").expect("address"),
        aggregate_version: 1,
        verified_at: delivery.occurred_at(),
    }
}

fn dispatcher(
    repository: Arc<InMemoryNotificationRepository>,
    contacts: Vec<Resolution>,
    preparations: Vec<
        Result<OutboundNotificationSmtpProviderOutcome, OutboundNotificationSmtpPreparationError>,
    >,
    phases: Arc<Mutex<Vec<&'static str>>>,
) -> OutboundNotificationSmtpDispatcher {
    OutboundNotificationSmtpDispatcher::new(
        Arc::new(RecordingAttempts {
            repository,
            phases: Arc::clone(&phases),
        }),
        Arc::new(ScriptedRecipientContacts::new(
            contacts,
            Arc::clone(&phases),
        )),
        Arc::new(ScriptedDeliveryService {
            preparations: Mutex::new(preparations.into()),
            phases,
        }),
        Duration::seconds(60),
        Duration::seconds(10),
    )
    .expect("SMTP dispatcher")
}

#[tokio::test]
async fn accepted_delivery_prepares_before_fencing_and_persists_before_return() {
    let (repository, delivery) = seeded_delivery(2).await;
    let contact = resolved_contact(&delivery);
    let phases = Arc::new(Mutex::new(Vec::new()));
    let dispatcher = dispatcher(
        Arc::clone(&repository),
        vec![Ok(Some(contact.clone())), Ok(Some(contact))],
        vec![Ok(OutboundNotificationSmtpProviderOutcome::Accepted)],
        Arc::clone(&phases),
    );
    let result = dispatcher.dispatch(&delivery, 1).await.expect("dispatch");
    assert!(matches!(
        result,
        OutboundNotificationDispatchResult::TerminalPersisted { receipt }
            if receipt.outcome() == OutboundNotificationTerminalOutcome::Delivered
    ));
    assert_eq!(
        *phases.lock().await,
        vec!["reserve", "resolve", "prepare", "resolve", "start", "deliver", "settle"]
    );
    assert_eq!(repository.outbound_receipts().await.len(), 1);
}

#[tokio::test]
async fn authority_loss_after_preparation_obsoletes_without_crossing_the_fence() {
    let (repository, delivery) = seeded_delivery(2).await;
    let phases = Arc::new(Mutex::new(Vec::new()));
    let dispatcher = dispatcher(
        Arc::clone(&repository),
        vec![Ok(Some(resolved_contact(&delivery))), Ok(None)],
        vec![Ok(OutboundNotificationSmtpProviderOutcome::Accepted)],
        Arc::clone(&phases),
    );
    let result = dispatcher.dispatch(&delivery, 1).await.expect("dispatch");
    assert!(matches!(
        result,
        OutboundNotificationDispatchResult::TerminalPersisted { receipt }
            if receipt.outcome() == OutboundNotificationTerminalOutcome::Obsolete
    ));
    assert_eq!(
        *phases.lock().await,
        vec!["reserve", "resolve", "prepare", "resolve", "settle"]
    );
}

#[tokio::test]
async fn retryable_evidence_advances_only_on_redelivery_and_exhausts_exact_budget() {
    let (repository, delivery) = seeded_delivery(2).await;
    let contact = resolved_contact(&delivery);
    let phases = Arc::new(Mutex::new(Vec::new()));
    let dispatcher = dispatcher(
        Arc::clone(&repository),
        vec![
            Ok(Some(contact.clone())),
            Ok(Some(contact.clone())),
            Ok(Some(contact.clone())),
            Ok(Some(contact)),
        ],
        vec![
            Ok(OutboundNotificationSmtpProviderOutcome::Retryable),
            Ok(OutboundNotificationSmtpProviderOutcome::Retryable),
        ],
        phases,
    );
    assert!(matches!(
        dispatcher
            .dispatch(&delivery, 1)
            .await
            .expect("first dispatch"),
        OutboundNotificationDispatchResult::SmtpRetryable { generation: 1, .. }
    ));
    assert!(repository.outbound_receipts().await.is_empty());
    assert!(matches!(
        dispatcher.dispatch(&delivery, 2).await.expect("second dispatch"),
        OutboundNotificationDispatchResult::TerminalPersisted { receipt }
            if receipt.outcome() == OutboundNotificationTerminalOutcome::Exhausted
                && receipt.generation() == 2
    ));
}

#[tokio::test]
async fn resolver_and_preparation_outages_remain_unacknowledged_before_fencing() {
    let (repository, delivery) = seeded_delivery(2).await;
    let phases = Arc::new(Mutex::new(Vec::new()));
    let resolver_outage = dispatcher(
        Arc::clone(&repository),
        vec![Err(RepositoryError::Storage("identity unavailable".into()))],
        Vec::new(),
        Arc::clone(&phases),
    );
    assert!(resolver_outage.dispatch(&delivery, 1).await.is_err());
    assert_eq!(*phases.lock().await, vec!["reserve", "resolve"]);

    let (repository, delivery) = seeded_delivery(2).await;
    let phases = Arc::new(Mutex::new(Vec::new()));
    let preparation_outage = dispatcher(
        repository,
        vec![Ok(Some(resolved_contact(&delivery)))],
        vec![Err(OutboundNotificationSmtpPreparationError::Unavailable)],
        Arc::clone(&phases),
    );
    assert!(matches!(
        preparation_outage
            .dispatch(&delivery, 1)
            .await
            .expect("deferred preparation"),
        OutboundNotificationDispatchResult::Deferred { generation: 1, .. }
    ));
    assert_eq!(*phases.lock().await, vec!["reserve", "resolve", "prepare"]);
}
