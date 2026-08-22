use crate::modules::identity::domain::entities::{
    RecipientContactVerificationDeliveryFact, RecipientContactVerificationDeliveryOutcome,
    RecipientContactVerificationDeliveryStatus,
};
use crate::modules::identity::domain::repositories::{
    IRecipientContactVerificationDeliveryRepository, RecipientContactVerificationDeliveryAdmission,
    RecipientContactVerificationDispatchStart,
};
use crate::modules::identity::domain::services::{
    IRecipientContactProofService, IRecipientContactVerificationDeliveryService,
    RecipientContactVerificationDeliveryRequest, RecipientContactVerificationProviderOutcome,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipientContactVerificationDispatchResult {
    Terminal(RecipientContactVerificationDeliveryStatus),
    Deferred,
    InvalidFact,
}

#[async_trait]
pub trait IRecipientContactVerificationDispatcher: Send + Sync {
    async fn dispatch(
        &self,
        fact: &RecipientContactVerificationDeliveryFact,
    ) -> Result<RecipientContactVerificationDispatchResult, String>;
}

pub struct RecipientContactVerificationDeliveryDispatcher {
    repository: Arc<dyn IRecipientContactVerificationDeliveryRepository>,
    proof_service: Arc<dyn IRecipientContactProofService>,
    delivery_service: Arc<dyn IRecipientContactVerificationDeliveryService>,
    reservation_lease: Duration,
}

impl RecipientContactVerificationDeliveryDispatcher {
    pub fn new(
        repository: Arc<dyn IRecipientContactVerificationDeliveryRepository>,
        proof_service: Arc<dyn IRecipientContactProofService>,
        delivery_service: Arc<dyn IRecipientContactVerificationDeliveryService>,
        reservation_lease: Duration,
    ) -> Result<Self, String> {
        if reservation_lease < Duration::seconds(30) || reservation_lease > Duration::minutes(5) {
            return Err(
                "recipient contact verification delivery lease must be 30 seconds to 5 minutes"
                    .into(),
            );
        }
        Ok(Self {
            repository,
            proof_service,
            delivery_service,
            reservation_lease,
        })
    }

    async fn dispatch_inner(
        &self,
        fact: &RecipientContactVerificationDeliveryFact,
    ) -> Result<RecipientContactVerificationDispatchResult, String> {
        if fact.validate().is_err() {
            return Ok(RecipientContactVerificationDispatchResult::InvalidFact);
        }
        let reserved_at = Utc::now();
        let fence_token = Uuid::now_v7();
        let reservation = match self
            .repository
            .reserve_recipient_contact_verification_delivery(
                fact,
                fence_token,
                reserved_at,
                reserved_at + self.reservation_lease,
            )
            .await
            .map_err(|error| error.to_string())?
        {
            RecipientContactVerificationDeliveryAdmission::Reserved(value) => value,
            RecipientContactVerificationDeliveryAdmission::Deferred { .. } => {
                return Ok(RecipientContactVerificationDispatchResult::Deferred)
            }
            RecipientContactVerificationDeliveryAdmission::Terminal(status) => {
                return Ok(RecipientContactVerificationDispatchResult::Terminal(status))
            }
            RecipientContactVerificationDeliveryAdmission::InvalidFact => {
                return Ok(RecipientContactVerificationDispatchResult::InvalidFact)
            }
        };

        let (proof, prepared) = tokio::join!(
            self.proof_service.issue(&reservation.verification),
            self.delivery_service.prepare(),
        );
        let (Ok(proof), Ok(prepared)) = (proof, prepared) else {
            return Ok(RecipientContactVerificationDispatchResult::Deferred);
        };
        match self
            .repository
            .start_recipient_contact_verification_dispatch(
                fact,
                reservation.fence_token,
                Utc::now(),
            )
            .await
            .map_err(|error| error.to_string())?
        {
            RecipientContactVerificationDispatchStart::Authorized => {}
            RecipientContactVerificationDispatchStart::Deferred => {
                return Ok(RecipientContactVerificationDispatchResult::Deferred)
            }
            RecipientContactVerificationDispatchStart::Terminal(status) => {
                return Ok(RecipientContactVerificationDispatchResult::Terminal(status))
            }
        }

        let provider_outcome = prepared
            .deliver(RecipientContactVerificationDeliveryRequest {
                verification_id: reservation.verification.id,
                address: reservation.address,
                proof,
                expires_at: reservation.verification.expires_at,
            })
            .await;
        let outcome = match provider_outcome {
            RecipientContactVerificationProviderOutcome::Delivered => {
                RecipientContactVerificationDeliveryOutcome::Delivered
            }
            RecipientContactVerificationProviderOutcome::Rejected => {
                RecipientContactVerificationDeliveryOutcome::Rejected
            }
            RecipientContactVerificationProviderOutcome::Indeterminate => {
                RecipientContactVerificationDeliveryOutcome::Indeterminate
            }
        };
        let record = self
            .repository
            .settle_recipient_contact_verification_delivery(
                fact.id(),
                reservation.fence_token,
                outcome,
                Utc::now(),
            )
            .await
            .map_err(|error| error.to_string())?;
        Ok(RecipientContactVerificationDispatchResult::Terminal(
            record.status,
        ))
    }
}

#[async_trait]
impl IRecipientContactVerificationDispatcher for RecipientContactVerificationDeliveryDispatcher {
    async fn dispatch(
        &self,
        fact: &RecipientContactVerificationDeliveryFact,
    ) -> Result<RecipientContactVerificationDispatchResult, String> {
        self.dispatch_inner(fact).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::{
        RecipientContactVerification, RecipientContactVerificationClaims,
        RecipientContactVerificationDeliveryRecord,
        RecipientContactVerificationDeliveryReservation,
    };
    use crate::modules::identity::domain::services::{
        IPreparedRecipientContactVerificationDelivery, RecipientContactProofError,
        RecipientContactVerificationDeliveryPreparationError,
    };
    use crate::modules::identity::domain::value_objects::{
        RecipientContactSigningKeyId, RecipientEmailAddress,
    };
    use crate::modules::shared_kernel::domain::{
        canonical_timestamp, OrganizationId, PrincipalId, RecipientContactId,
        RecipientContactVerificationId, RepositoryError, Sha256Digest,
    };
    use chrono::DateTime;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use zeroize::Zeroizing;

    struct ScriptedRepository {
        admission: Mutex<RecipientContactVerificationDeliveryAdmission>,
        start: RecipientContactVerificationDispatchStart,
        now: DateTime<Utc>,
        reserve_calls: AtomicUsize,
        start_calls: AtomicUsize,
        settle_calls: AtomicUsize,
        settled_outcomes: Mutex<Vec<RecipientContactVerificationDeliveryOutcome>>,
    }

    #[async_trait]
    impl IRecipientContactVerificationDeliveryRepository for ScriptedRepository {
        async fn reserve_recipient_contact_verification_delivery(
            &self,
            _fact: &RecipientContactVerificationDeliveryFact,
            _fence_token: Uuid,
            _reserved_at: DateTime<Utc>,
            _lease_expires_at: DateTime<Utc>,
        ) -> Result<RecipientContactVerificationDeliveryAdmission, RepositoryError> {
            self.reserve_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.admission.lock().expect("admission lock").clone())
        }

        async fn start_recipient_contact_verification_dispatch(
            &self,
            _fact: &RecipientContactVerificationDeliveryFact,
            _fence_token: Uuid,
            _started_at: DateTime<Utc>,
        ) -> Result<RecipientContactVerificationDispatchStart, RepositoryError> {
            self.start_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.start)
        }

        async fn settle_recipient_contact_verification_delivery(
            &self,
            verification_id: RecipientContactVerificationId,
            fence_token: Uuid,
            outcome: RecipientContactVerificationDeliveryOutcome,
            _settled_at: DateTime<Utc>,
        ) -> Result<RecipientContactVerificationDeliveryRecord, RepositoryError> {
            self.settle_calls.fetch_add(1, Ordering::SeqCst);
            self.settled_outcomes
                .lock()
                .expect("settlement lock")
                .push(outcome);
            RecipientContactVerificationDeliveryRecord::restore(
                verification_id,
                outcome.status(),
                fence_token,
                self.now,
                self.now + Duration::minutes(1),
                Some(self.now + Duration::seconds(1)),
                Some(self.now + Duration::seconds(2)),
            )
            .map_err(RepositoryError::Storage)
        }

        async fn find_recipient_contact_verification_delivery(
            &self,
            _verification_id: RecipientContactVerificationId,
        ) -> Result<Option<RecipientContactVerificationDeliveryRecord>, RepositoryError> {
            Ok(None)
        }
    }

    struct ScriptedProofService {
        key_id: RecipientContactSigningKeyId,
        succeeds: bool,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl IRecipientContactProofService for ScriptedProofService {
        fn current_key_id(&self) -> &RecipientContactSigningKeyId {
            &self.key_id
        }

        async fn issue(
            &self,
            _verification: &RecipientContactVerification,
        ) -> Result<Zeroizing<String>, RecipientContactProofError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.succeeds {
                Ok(Zeroizing::new("a3srcv1.synthetic.proof".into()))
            } else {
                Err(RecipientContactProofError::Unavailable)
            }
        }

        async fn verify(
            &self,
            _proof: &str,
            _now: DateTime<Utc>,
        ) -> Result<RecipientContactVerificationClaims, RecipientContactProofError> {
            Err(RecipientContactProofError::Rejected)
        }
    }

    struct ScriptedDeliveryService {
        prepare_succeeds: bool,
        outcome: RecipientContactVerificationProviderOutcome,
        prepare_calls: AtomicUsize,
        deliver_calls: Arc<AtomicUsize>,
    }

    struct ScriptedPreparedDelivery {
        outcome: RecipientContactVerificationProviderOutcome,
        deliver_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl IRecipientContactVerificationDeliveryService for ScriptedDeliveryService {
        async fn prepare(
            &self,
        ) -> Result<
            Box<dyn IPreparedRecipientContactVerificationDelivery>,
            RecipientContactVerificationDeliveryPreparationError,
        > {
            self.prepare_calls.fetch_add(1, Ordering::SeqCst);
            if !self.prepare_succeeds {
                return Err(RecipientContactVerificationDeliveryPreparationError::Unavailable);
            }
            Ok(Box::new(ScriptedPreparedDelivery {
                outcome: self.outcome,
                deliver_calls: Arc::clone(&self.deliver_calls),
            }))
        }
    }

    #[async_trait]
    impl IPreparedRecipientContactVerificationDelivery for ScriptedPreparedDelivery {
        async fn deliver(
            self: Box<Self>,
            _request: RecipientContactVerificationDeliveryRequest,
        ) -> RecipientContactVerificationProviderOutcome {
            self.deliver_calls.fetch_add(1, Ordering::SeqCst);
            self.outcome
        }
    }

    fn fact(now: DateTime<Utc>) -> RecipientContactVerificationDeliveryFact {
        RecipientContactVerificationDeliveryFact {
            organization_id: OrganizationId::new(),
            verification: RecipientContactVerification::create(
                RecipientContactVerificationId::new(),
                RecipientContactId::new(),
                PrincipalId::new(),
                Sha256Digest::from_bytes(b"recipient"),
                1,
                RecipientContactSigningKeyId::parse("contact-v1").expect("key ID"),
                now,
                now + Duration::minutes(10),
            )
            .expect("verification"),
        }
    }

    fn repository(
        fact: &RecipientContactVerificationDeliveryFact,
        now: DateTime<Utc>,
        admission_status: Option<RecipientContactVerificationDeliveryStatus>,
        start: RecipientContactVerificationDispatchStart,
    ) -> Arc<ScriptedRepository> {
        let fence_token = Uuid::now_v7();
        let admission = match admission_status {
            Some(status) => RecipientContactVerificationDeliveryAdmission::Terminal(status),
            None => RecipientContactVerificationDeliveryAdmission::Reserved(
                RecipientContactVerificationDeliveryReservation {
                    verification: fact.verification.clone(),
                    address: RecipientEmailAddress::parse("private@example.test").expect("address"),
                    fence_token,
                    lease_expires_at: now + Duration::minutes(1),
                },
            ),
        };
        Arc::new(ScriptedRepository {
            admission: Mutex::new(admission),
            start,
            now,
            reserve_calls: AtomicUsize::new(0),
            start_calls: AtomicUsize::new(0),
            settle_calls: AtomicUsize::new(0),
            settled_outcomes: Mutex::new(Vec::new()),
        })
    }

    fn proof_service(succeeds: bool) -> Arc<ScriptedProofService> {
        Arc::new(ScriptedProofService {
            key_id: RecipientContactSigningKeyId::parse("contact-v1").expect("key ID"),
            succeeds,
            calls: AtomicUsize::new(0),
        })
    }

    fn delivery_service(
        prepare_succeeds: bool,
        outcome: RecipientContactVerificationProviderOutcome,
    ) -> Arc<ScriptedDeliveryService> {
        Arc::new(ScriptedDeliveryService {
            prepare_succeeds,
            outcome,
            prepare_calls: AtomicUsize::new(0),
            deliver_calls: Arc::new(AtomicUsize::new(0)),
        })
    }

    fn dispatcher(
        repository: Arc<ScriptedRepository>,
        proof: Arc<ScriptedProofService>,
        delivery: Arc<ScriptedDeliveryService>,
    ) -> RecipientContactVerificationDeliveryDispatcher {
        RecipientContactVerificationDeliveryDispatcher::new(
            repository,
            proof,
            delivery,
            Duration::minutes(1),
        )
        .expect("dispatcher")
    }

    #[tokio::test]
    async fn pre_fence_unavailability_never_calls_the_provider() {
        let now = canonical_timestamp(Utc::now());
        let fact = fact(now);
        for (proof_succeeds, prepare_succeeds) in [(false, true), (true, false)] {
            let repository = repository(
                &fact,
                now,
                None,
                RecipientContactVerificationDispatchStart::Authorized,
            );
            let proof = proof_service(proof_succeeds);
            let delivery = delivery_service(
                prepare_succeeds,
                RecipientContactVerificationProviderOutcome::Delivered,
            );
            assert_eq!(
                dispatcher(
                    Arc::clone(&repository),
                    Arc::clone(&proof),
                    Arc::clone(&delivery),
                )
                .dispatch(&fact)
                .await
                .expect("dispatch"),
                RecipientContactVerificationDispatchResult::Deferred
            );
            assert_eq!(repository.start_calls.load(Ordering::SeqCst), 0);
            assert_eq!(repository.settle_calls.load(Ordering::SeqCst), 0);
            assert_eq!(delivery.deliver_calls.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn authority_is_rechecked_after_preparation_before_any_submission() {
        let now = canonical_timestamp(Utc::now());
        let fact = fact(now);
        let repository = repository(
            &fact,
            now,
            None,
            RecipientContactVerificationDispatchStart::Terminal(
                RecipientContactVerificationDeliveryStatus::Obsolete,
            ),
        );
        let proof = proof_service(true);
        let delivery =
            delivery_service(true, RecipientContactVerificationProviderOutcome::Delivered);
        assert_eq!(
            dispatcher(
                Arc::clone(&repository),
                Arc::clone(&proof),
                Arc::clone(&delivery),
            )
            .dispatch(&fact)
            .await
            .expect("dispatch"),
            RecipientContactVerificationDispatchResult::Terminal(
                RecipientContactVerificationDeliveryStatus::Obsolete
            )
        );
        assert_eq!(proof.calls.load(Ordering::SeqCst), 1);
        assert_eq!(delivery.prepare_calls.load(Ordering::SeqCst), 1);
        assert_eq!(delivery.deliver_calls.load(Ordering::SeqCst), 0);
        assert_eq!(repository.settle_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn one_authorized_provider_call_is_settled_to_the_exact_terminal_outcome() {
        let now = canonical_timestamp(Utc::now());
        let fact = fact(now);
        for (provider_outcome, expected_status, expected_outcome) in [
            (
                RecipientContactVerificationProviderOutcome::Delivered,
                RecipientContactVerificationDeliveryStatus::Delivered,
                RecipientContactVerificationDeliveryOutcome::Delivered,
            ),
            (
                RecipientContactVerificationProviderOutcome::Rejected,
                RecipientContactVerificationDeliveryStatus::Rejected,
                RecipientContactVerificationDeliveryOutcome::Rejected,
            ),
            (
                RecipientContactVerificationProviderOutcome::Indeterminate,
                RecipientContactVerificationDeliveryStatus::Indeterminate,
                RecipientContactVerificationDeliveryOutcome::Indeterminate,
            ),
        ] {
            let repository = repository(
                &fact,
                now,
                None,
                RecipientContactVerificationDispatchStart::Authorized,
            );
            let proof = proof_service(true);
            let delivery = delivery_service(true, provider_outcome);
            assert_eq!(
                dispatcher(
                    Arc::clone(&repository),
                    Arc::clone(&proof),
                    Arc::clone(&delivery),
                )
                .dispatch(&fact)
                .await
                .expect("dispatch"),
                RecipientContactVerificationDispatchResult::Terminal(expected_status)
            );
            assert_eq!(repository.start_calls.load(Ordering::SeqCst), 1);
            assert_eq!(delivery.deliver_calls.load(Ordering::SeqCst), 1);
            assert_eq!(repository.settle_calls.load(Ordering::SeqCst), 1);
            assert_eq!(
                repository
                    .settled_outcomes
                    .lock()
                    .expect("settlements")
                    .as_slice(),
                &[expected_outcome]
            );
        }
    }

    #[tokio::test]
    async fn terminal_replay_is_ack_only_and_skips_sensitive_preparation() {
        let now = canonical_timestamp(Utc::now());
        let fact = fact(now);
        let repository = repository(
            &fact,
            now,
            Some(RecipientContactVerificationDeliveryStatus::Delivered),
            RecipientContactVerificationDispatchStart::Authorized,
        );
        let proof = proof_service(true);
        let delivery =
            delivery_service(true, RecipientContactVerificationProviderOutcome::Delivered);
        assert_eq!(
            dispatcher(
                Arc::clone(&repository),
                Arc::clone(&proof),
                Arc::clone(&delivery),
            )
            .dispatch(&fact)
            .await
            .expect("terminal replay"),
            RecipientContactVerificationDispatchResult::Terminal(
                RecipientContactVerificationDeliveryStatus::Delivered
            )
        );
        assert_eq!(repository.reserve_calls.load(Ordering::SeqCst), 1);
        assert_eq!(proof.calls.load(Ordering::SeqCst), 0);
        assert_eq!(delivery.prepare_calls.load(Ordering::SeqCst), 0);
        assert_eq!(delivery.deliver_calls.load(Ordering::SeqCst), 0);
        assert_eq!(repository.start_calls.load(Ordering::SeqCst), 0);
        assert_eq!(repository.settle_calls.load(Ordering::SeqCst), 0);
    }
}
