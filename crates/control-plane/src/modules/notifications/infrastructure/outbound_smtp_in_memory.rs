use super::in_memory::{InMemoryNotificationRepository, State};
use crate::modules::notifications::domain::{
    outbound_notification_smtp_attempt_id, IOutboundNotificationSmtpAttemptRepository,
    OutboundNotificationChannel, OutboundNotificationDelivery,
    OutboundNotificationSmtpAttemptAdmission, OutboundNotificationSmtpAttemptOutcome,
    OutboundNotificationSmtpAttemptRecord, OutboundNotificationSmtpAttemptSettlement,
    OutboundNotificationSmtpAttemptState, OutboundNotificationSmtpDispatchStart,
    OutboundNotificationTerminalReceipt, MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_LEASE_SECONDS,
};
use crate::modules::shared_kernel::domain::{canonical_timestamp, RepositoryError, Sha256Digest};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

#[async_trait]
impl IOutboundNotificationSmtpAttemptRepository for InMemoryNotificationRepository {
    async fn reserve_smtp_attempt(
        &self,
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        fence_token: Uuid,
        reserved_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<OutboundNotificationSmtpAttemptAdmission, RepositoryError> {
        if !valid_reservation(
            delivery,
            generation,
            fence_token,
            reserved_at,
            lease_expires_at,
        ) {
            return Ok(OutboundNotificationSmtpAttemptAdmission::InvalidFact);
        }
        let mut state = self.state.write().await;
        let receipt = match validate_delivery(&state, delivery) {
            Ok(receipt) => receipt,
            Err(RepositoryError::NotFound | RepositoryError::Conflict(_)) => {
                return Ok(OutboundNotificationSmtpAttemptAdmission::InvalidFact)
            }
            Err(error) => return Err(error),
        };
        if let Some(receipt) = receipt {
            return Ok(OutboundNotificationSmtpAttemptAdmission::Terminal(receipt));
        }

        let key = (delivery.organization_id(), delivery.id(), generation);
        if let Some(existing) = state.outbound_smtp_attempts.get(&key).cloned() {
            validate_attempt_against_delivery(&existing, delivery)?;
            return match existing.state {
                OutboundNotificationSmtpAttemptState::Reserved => {
                    if reserved_at < existing.lease_expires_at {
                        Ok(OutboundNotificationSmtpAttemptAdmission::Deferred {
                            retry_not_before: existing.lease_expires_at,
                        })
                    } else {
                        let replacement = OutboundNotificationSmtpAttemptRecord::restore(
                            existing.organization_id,
                            existing.delivery_id,
                            existing.recipient_contact_id,
                            existing.generation,
                            existing.attempt_id,
                            OutboundNotificationSmtpAttemptState::Reserved,
                            None,
                            existing.fence_generation.checked_add(1).ok_or_else(|| {
                                RepositoryError::Storage(
                                    "outbound SMTP notification fence generation overflowed".into(),
                                )
                            })?,
                            fence_token,
                            reserved_at,
                            lease_expires_at,
                            None,
                            None,
                            None,
                        )
                        .map_err(RepositoryError::Storage)?;
                        state
                            .outbound_smtp_attempts
                            .insert(key, replacement.clone());
                        Ok(OutboundNotificationSmtpAttemptAdmission::Reserved(
                            replacement,
                        ))
                    }
                }
                OutboundNotificationSmtpAttemptState::Dispatching => {
                    let deadline = existing.outcome_deadline_at.ok_or_else(|| {
                        RepositoryError::Storage(
                            "dispatching SMTP notification attempt has no outcome deadline".into(),
                        )
                    })?;
                    if reserved_at < deadline {
                        Ok(OutboundNotificationSmtpAttemptAdmission::Deferred {
                            retry_not_before: deadline,
                        })
                    } else {
                        let receipt =
                            recover_indeterminate(&mut state, delivery, existing, deadline)?;
                        Ok(OutboundNotificationSmtpAttemptAdmission::Terminal(receipt))
                    }
                }
                OutboundNotificationSmtpAttemptState::Terminal => {
                    terminal_admission(&state, delivery, existing)
                }
            };
        }

        validate_new_generation(&state, delivery, generation)?;
        let record = OutboundNotificationSmtpAttemptRecord::restore(
            delivery.organization_id(),
            delivery.id(),
            delivery.recipient_contact_id().ok_or_else(|| {
                RepositoryError::Storage("SMTP notification delivery has no contact target".into())
            })?,
            generation,
            outbound_notification_smtp_attempt_id(delivery.id(), generation)
                .map_err(RepositoryError::Storage)?,
            OutboundNotificationSmtpAttemptState::Reserved,
            None,
            1,
            fence_token,
            reserved_at,
            lease_expires_at,
            None,
            None,
            None,
        )
        .map_err(RepositoryError::Storage)?;
        state.outbound_smtp_attempts.insert(key, record.clone());
        Ok(OutboundNotificationSmtpAttemptAdmission::Reserved(record))
    }

    async fn start_smtp_dispatch(
        &self,
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        fence_token: Uuid,
        started_at: DateTime<Utc>,
        outcome_deadline_at: DateTime<Utc>,
    ) -> Result<OutboundNotificationSmtpDispatchStart, RepositoryError> {
        delivery.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        if let Some(receipt) = validate_delivery(&state, delivery)? {
            return Ok(OutboundNotificationSmtpDispatchStart::Terminal(receipt));
        }
        let key = (delivery.organization_id(), delivery.id(), generation);
        let existing = state
            .outbound_smtp_attempts
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        validate_attempt_against_delivery(&existing, delivery)?;

        match existing.state {
            OutboundNotificationSmtpAttemptState::Reserved => {
                if existing.fence_token != fence_token || started_at >= existing.lease_expires_at {
                    return Ok(OutboundNotificationSmtpDispatchStart::Deferred {
                        retry_not_before: existing.lease_expires_at,
                    });
                }
                let record = OutboundNotificationSmtpAttemptRecord::restore(
                    existing.organization_id,
                    existing.delivery_id,
                    existing.recipient_contact_id,
                    existing.generation,
                    existing.attempt_id,
                    OutboundNotificationSmtpAttemptState::Dispatching,
                    None,
                    existing.fence_generation,
                    existing.fence_token,
                    existing.reserved_at,
                    existing.lease_expires_at,
                    Some(started_at),
                    Some(outcome_deadline_at),
                    None,
                )
                .map_err(RepositoryError::Storage)?;
                state.outbound_smtp_attempts.insert(key, record.clone());
                Ok(OutboundNotificationSmtpDispatchStart::Authorized(record))
            }
            OutboundNotificationSmtpAttemptState::Dispatching => {
                let deadline = existing.outcome_deadline_at.ok_or_else(|| {
                    RepositoryError::Storage(
                        "dispatching SMTP notification attempt has no outcome deadline".into(),
                    )
                })?;
                if started_at < deadline {
                    Ok(OutboundNotificationSmtpDispatchStart::Deferred {
                        retry_not_before: deadline,
                    })
                } else {
                    let receipt = recover_indeterminate(&mut state, delivery, existing, deadline)?;
                    Ok(OutboundNotificationSmtpDispatchStart::Terminal(receipt))
                }
            }
            OutboundNotificationSmtpAttemptState::Terminal => {
                terminal_start(&state, delivery, existing)
            }
        }
    }

    async fn settle_smtp_attempt(
        &self,
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        fence_token: Uuid,
        outcome: OutboundNotificationSmtpAttemptOutcome,
        settled_at: DateTime<Utc>,
    ) -> Result<OutboundNotificationSmtpAttemptSettlement, RepositoryError> {
        delivery.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let existing_receipt = validate_delivery(&state, delivery)?;
        let key = (delivery.organization_id(), delivery.id(), generation);
        let existing = state
            .outbound_smtp_attempts
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        validate_attempt_against_delivery(&existing, delivery)?;
        if let Some(receipt) = existing_receipt {
            return Ok(OutboundNotificationSmtpAttemptSettlement {
                attempt: existing,
                receipt: Some(receipt),
            });
        }
        if existing.state == OutboundNotificationSmtpAttemptState::Terminal {
            if existing.fence_token != fence_token || existing.outcome != Some(outcome) {
                return Err(RepositoryError::Conflict(
                    "terminal SMTP notification attempt differs from its replay".into(),
                ));
            }
            let receipt = terminal_receipt(delivery, &existing)?;
            return Ok(OutboundNotificationSmtpAttemptSettlement {
                attempt: existing,
                receipt,
            });
        }
        if existing.fence_token != fence_token {
            return Err(RepositoryError::Conflict(
                "SMTP notification settlement uses a stale dispatch fence".into(),
            ));
        }

        let (dispatch_started_at, outcome_deadline_at) = match existing.state {
            OutboundNotificationSmtpAttemptState::Reserved
                if outcome == OutboundNotificationSmtpAttemptOutcome::Obsolete
                    && settled_at >= existing.reserved_at
                    && settled_at <= existing.lease_expires_at =>
            {
                (None, None)
            }
            OutboundNotificationSmtpAttemptState::Dispatching
                if outcome != OutboundNotificationSmtpAttemptOutcome::Obsolete =>
            {
                (existing.dispatch_started_at, existing.outcome_deadline_at)
            }
            _ => {
                return Err(RepositoryError::Conflict(
                    "SMTP notification attempt cannot settle from its current state".into(),
                ))
            }
        };
        let record = OutboundNotificationSmtpAttemptRecord::restore(
            existing.organization_id,
            existing.delivery_id,
            existing.recipient_contact_id,
            existing.generation,
            existing.attempt_id,
            OutboundNotificationSmtpAttemptState::Terminal,
            Some(outcome),
            existing.fence_generation,
            existing.fence_token,
            existing.reserved_at,
            existing.lease_expires_at,
            dispatch_started_at,
            outcome_deadline_at,
            Some(settled_at),
        )
        .map_err(RepositoryError::Storage)?;
        let receipt = terminal_receipt(delivery, &record)?;
        if let Some(receipt) = receipt.clone() {
            store_receipt(&mut state, delivery, receipt)?;
        }
        state.outbound_smtp_attempts.insert(key, record.clone());
        Ok(OutboundNotificationSmtpAttemptSettlement {
            attempt: record,
            receipt,
        })
    }

    async fn find_smtp_attempt(
        &self,
        organization_id: crate::modules::shared_kernel::domain::OrganizationId,
        delivery_id: Uuid,
        generation: u64,
    ) -> Result<Option<OutboundNotificationSmtpAttemptRecord>, RepositoryError> {
        if delivery_id.is_nil() || generation == 0 {
            return Err(RepositoryError::Storage(
                "outbound SMTP notification attempt lookup is invalid".into(),
            ));
        }
        Ok(self
            .state
            .read()
            .await
            .outbound_smtp_attempts
            .get(&(organization_id, delivery_id, generation))
            .cloned())
    }
}

fn valid_reservation(
    delivery: &OutboundNotificationDelivery,
    generation: u64,
    fence_token: Uuid,
    reserved_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
) -> bool {
    delivery.validate().is_ok()
        && delivery.channel() == OutboundNotificationChannel::Smtp
        && delivery.recipient_contact_id().is_some()
        && generation > 0
        && generation <= delivery.maximum_provider_attempts()
        && !fence_token.is_nil()
        && reserved_at == canonical_timestamp(reserved_at)
        && lease_expires_at == canonical_timestamp(lease_expires_at)
        && reserved_at >= delivery.occurred_at()
        && lease_expires_at > reserved_at
        && lease_expires_at - reserved_at
            <= Duration::seconds(MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_LEASE_SECONDS)
}

fn validate_delivery(
    state: &State,
    delivery: &OutboundNotificationDelivery,
) -> Result<Option<OutboundNotificationTerminalReceipt>, RepositoryError> {
    if delivery.channel() != OutboundNotificationChannel::Smtp
        || delivery.recipient_contact_id().is_none()
    {
        return Err(RepositoryError::Conflict(
            "SMTP attempt requires an exact recipient-contact delivery".into(),
        ));
    }
    let stored = state
        .outbound_deliveries
        .get(&(delivery.organization_id(), delivery.id()))
        .ok_or(RepositoryError::NotFound)?;
    let payload_digest = Sha256Digest::from_bytes(
        &delivery
            .canonical_payload()
            .map_err(RepositoryError::Storage)?,
    );
    if stored.delivery != *delivery
        || stored.payload_digest != payload_digest
        || !state
            .subscriptions
            .contains_key(&(delivery.organization_id(), stored.subscription_id))
    {
        return Err(RepositoryError::Conflict(
            "SMTP attempt delivery fact is not authorized".into(),
        ));
    }
    if let Some(receipt) = &stored.receipt {
        receipt
            .validate_against(delivery)
            .map_err(RepositoryError::Storage)?;
    }
    Ok(stored.receipt.clone())
}

fn validate_attempt_against_delivery(
    attempt: &OutboundNotificationSmtpAttemptRecord,
    delivery: &OutboundNotificationDelivery,
) -> Result<(), RepositoryError> {
    attempt.validate().map_err(RepositoryError::Storage)?;
    if attempt.organization_id != delivery.organization_id()
        || attempt.delivery_id != delivery.id()
        || Some(attempt.recipient_contact_id) != delivery.recipient_contact_id()
        || attempt.generation > delivery.maximum_provider_attempts()
    {
        return Err(RepositoryError::Conflict(
            "SMTP attempt does not match its exact delivery".into(),
        ));
    }
    Ok(())
}

fn validate_new_generation(
    state: &State,
    delivery: &OutboundNotificationDelivery,
    generation: u64,
) -> Result<(), RepositoryError> {
    let attempts = state.outbound_smtp_attempts.values().filter(|attempt| {
        attempt.organization_id == delivery.organization_id()
            && attempt.delivery_id == delivery.id()
    });
    let mut prior = None;
    for attempt in attempts {
        if attempt.generation >= generation {
            return Err(RepositoryError::Conflict(
                "SMTP attempt generation is not monotonic".into(),
            ));
        }
        if attempt.generation + 1 == generation {
            prior = Some(attempt);
        }
    }
    if generation == 1 {
        return Ok(());
    }
    if prior.is_some_and(|attempt| {
        attempt.state == OutboundNotificationSmtpAttemptState::Terminal
            && attempt.outcome == Some(OutboundNotificationSmtpAttemptOutcome::Retryable)
    }) {
        Ok(())
    } else {
        Err(RepositoryError::Conflict(
            "SMTP attempt requires exact prior retryable evidence".into(),
        ))
    }
}

fn terminal_receipt(
    delivery: &OutboundNotificationDelivery,
    attempt: &OutboundNotificationSmtpAttemptRecord,
) -> Result<Option<OutboundNotificationTerminalReceipt>, RepositoryError> {
    let outcome = attempt.outcome.ok_or_else(|| {
        RepositoryError::Storage("terminal SMTP notification attempt has no outcome".into())
    })?;
    let completed_at = attempt.completed_at.ok_or_else(|| {
        RepositoryError::Storage("terminal SMTP notification attempt has no completion time".into())
    })?;
    OutboundNotificationTerminalReceipt::from_smtp_outcome(
        delivery,
        attempt.generation,
        outcome,
        completed_at,
    )
    .map_err(RepositoryError::Storage)
}

fn terminal_admission(
    state: &State,
    delivery: &OutboundNotificationDelivery,
    attempt: OutboundNotificationSmtpAttemptRecord,
) -> Result<OutboundNotificationSmtpAttemptAdmission, RepositoryError> {
    let expected = terminal_receipt(delivery, &attempt)?;
    match expected {
        Some(receipt) => {
            let stored = validate_delivery(state, delivery)?.ok_or_else(|| {
                RepositoryError::Storage(
                    "terminal SMTP attempt has no atomic delivery receipt".into(),
                )
            })?;
            if stored != receipt {
                return Err(RepositoryError::Storage(
                    "terminal SMTP attempt and delivery receipt differ".into(),
                ));
            }
            Ok(OutboundNotificationSmtpAttemptAdmission::Terminal(stored))
        }
        None => Ok(OutboundNotificationSmtpAttemptAdmission::Retryable(attempt)),
    }
}

fn terminal_start(
    state: &State,
    delivery: &OutboundNotificationDelivery,
    attempt: OutboundNotificationSmtpAttemptRecord,
) -> Result<OutboundNotificationSmtpDispatchStart, RepositoryError> {
    let receipt = terminal_receipt(delivery, &attempt)?.ok_or_else(|| {
        RepositoryError::Conflict("retryable SMTP attempt cannot restart dispatch".into())
    })?;
    let stored = validate_delivery(state, delivery)?.ok_or_else(|| {
        RepositoryError::Storage("terminal SMTP attempt has no atomic delivery receipt".into())
    })?;
    if stored != receipt {
        return Err(RepositoryError::Storage(
            "terminal SMTP attempt and delivery receipt differ".into(),
        ));
    }
    Ok(OutboundNotificationSmtpDispatchStart::Terminal(stored))
}

fn recover_indeterminate(
    state: &mut State,
    delivery: &OutboundNotificationDelivery,
    attempt: OutboundNotificationSmtpAttemptRecord,
    deadline: DateTime<Utc>,
) -> Result<OutboundNotificationTerminalReceipt, RepositoryError> {
    let terminal = OutboundNotificationSmtpAttemptRecord::restore(
        attempt.organization_id,
        attempt.delivery_id,
        attempt.recipient_contact_id,
        attempt.generation,
        attempt.attempt_id,
        OutboundNotificationSmtpAttemptState::Terminal,
        Some(OutboundNotificationSmtpAttemptOutcome::Indeterminate),
        attempt.fence_generation,
        attempt.fence_token,
        attempt.reserved_at,
        attempt.lease_expires_at,
        attempt.dispatch_started_at,
        attempt.outcome_deadline_at,
        Some(deadline),
    )
    .map_err(RepositoryError::Storage)?;
    let receipt = terminal_receipt(delivery, &terminal)?.ok_or_else(|| {
        RepositoryError::Storage("indeterminate SMTP attempt produced no receipt".into())
    })?;
    store_receipt(state, delivery, receipt.clone())?;
    state.outbound_smtp_attempts.insert(
        (
            delivery.organization_id(),
            delivery.id(),
            attempt.generation,
        ),
        terminal,
    );
    Ok(receipt)
}

fn store_receipt(
    state: &mut State,
    delivery: &OutboundNotificationDelivery,
    receipt: OutboundNotificationTerminalReceipt,
) -> Result<(), RepositoryError> {
    receipt
        .validate_against(delivery)
        .map_err(RepositoryError::Storage)?;
    let stored = state
        .outbound_deliveries
        .get_mut(&(delivery.organization_id(), delivery.id()))
        .ok_or(RepositoryError::NotFound)?;
    match &stored.receipt {
        Some(existing) if existing == &receipt => Ok(()),
        Some(_) => Err(RepositoryError::Conflict(
            "SMTP delivery already has another terminal receipt".into(),
        )),
        None => {
            stored.receipt = Some(receipt);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::in_memory::StoredOutboundDelivery;
    use super::*;
    use crate::modules::notifications::domain::{
        Notification, NotificationScope, NotificationSeverity, OutboundNotificationSubscription,
        OutboundNotificationSubscriptionDefinition, OutboundNotificationTerminalOutcome,
    };
    use crate::modules::shared_kernel::domain::{
        NotificationSubscriptionId, OrganizationId, PrincipalId, RecipientContactId,
    };

    async fn seeded(
        maximum_provider_attempts: u64,
    ) -> (InMemoryNotificationRepository, OutboundNotificationDelivery) {
        let repository = InMemoryNotificationRepository::new();
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let occurred_at = canonical_timestamp(Utc::now());
        let definition = OutboundNotificationSubscriptionDefinition::from_smtp_spec(
            RecipientContactId::new(),
            NotificationSeverity::Information,
            maximum_provider_attempts,
            None,
        )
        .expect("SMTP subscription definition");
        let subscription = OutboundNotificationSubscription::create(
            organization_id,
            NotificationSubscriptionId::new(),
            principal_id,
            definition.clone(),
            principal_id,
            occurred_at,
        )
        .expect("SMTP subscription");
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
        let delivery = definition.delivery_for(&notification).expect("delivery");
        let payload_digest =
            Sha256Digest::from_bytes(&delivery.canonical_payload().expect("canonical delivery"));
        {
            let mut state = repository.state.write().await;
            state.subscriptions.insert(
                (subscription.organization_id, subscription.id),
                subscription.clone(),
            );
            state.outbound_deliveries.insert(
                (delivery.organization_id(), delivery.id()),
                StoredOutboundDelivery {
                    delivery: delivery.clone(),
                    subscription_id: subscription.id,
                    payload_digest,
                    receipt: None,
                },
            );
        }
        (repository, delivery)
    }

    #[tokio::test]
    async fn exact_retryable_generations_end_in_atomic_exhaustion() {
        let (repository, delivery) = seeded(2).await;
        let first_reserved = delivery.occurred_at() + Duration::seconds(1);
        let first_token = Uuid::now_v7();
        let first = repository
            .reserve_smtp_attempt(
                &delivery,
                1,
                first_token,
                first_reserved,
                first_reserved + Duration::seconds(30),
            )
            .await
            .expect("reserve first attempt");
        assert!(matches!(
            first,
            OutboundNotificationSmtpAttemptAdmission::Reserved(_)
        ));
        repository
            .start_smtp_dispatch(
                &delivery,
                1,
                first_token,
                first_reserved + Duration::seconds(1),
                first_reserved + Duration::seconds(11),
            )
            .await
            .expect("start first attempt");
        let first = repository
            .settle_smtp_attempt(
                &delivery,
                1,
                first_token,
                OutboundNotificationSmtpAttemptOutcome::Retryable,
                first_reserved + Duration::seconds(2),
            )
            .await
            .expect("settle first attempt");
        assert!(first.receipt.is_none());

        let second_reserved = first_reserved + Duration::seconds(3);
        let second_token = Uuid::now_v7();
        repository
            .reserve_smtp_attempt(
                &delivery,
                2,
                second_token,
                second_reserved,
                second_reserved + Duration::seconds(30),
            )
            .await
            .expect("reserve second attempt");
        repository
            .start_smtp_dispatch(
                &delivery,
                2,
                second_token,
                second_reserved + Duration::seconds(1),
                second_reserved + Duration::seconds(11),
            )
            .await
            .expect("start second attempt");
        let final_settlement = repository
            .settle_smtp_attempt(
                &delivery,
                2,
                second_token,
                OutboundNotificationSmtpAttemptOutcome::Retryable,
                second_reserved + Duration::seconds(2),
            )
            .await
            .expect("settle final attempt");
        let receipt = final_settlement.receipt.expect("exhausted receipt");
        assert_eq!(
            receipt.outcome(),
            OutboundNotificationTerminalOutcome::Exhausted
        );
        assert_eq!(repository.outbound_receipts().await, vec![receipt.clone()]);
        assert!(matches!(
            repository
                .reserve_smtp_attempt(
                    &delivery,
                    1,
                    Uuid::now_v7(),
                    second_reserved + Duration::seconds(4),
                    second_reserved + Duration::seconds(34),
                )
                .await
                .expect("terminal replay"),
            OutboundNotificationSmtpAttemptAdmission::Terminal(replayed) if replayed == receipt
        ));
    }

    #[tokio::test]
    async fn expired_reservations_are_fenced_and_dispatch_replay_is_indeterminate() {
        let (repository, delivery) = seeded(3).await;
        let first_reserved = delivery.occurred_at() + Duration::seconds(1);
        let first_token = Uuid::now_v7();
        let first_expiry = first_reserved + Duration::seconds(30);
        repository
            .reserve_smtp_attempt(&delivery, 1, first_token, first_reserved, first_expiry)
            .await
            .expect("first reservation");
        assert!(matches!(
            repository
                .reserve_smtp_attempt(
                    &delivery,
                    1,
                    Uuid::now_v7(),
                    first_reserved + Duration::seconds(10),
                    first_reserved + Duration::seconds(40),
                )
                .await
                .expect("live reservation replay"),
            OutboundNotificationSmtpAttemptAdmission::Deferred { retry_not_before }
                if retry_not_before == first_expiry
        ));

        let second_token = Uuid::now_v7();
        let second_expiry = first_expiry + Duration::seconds(30);
        let takeover = repository
            .reserve_smtp_attempt(&delivery, 1, second_token, first_expiry, second_expiry)
            .await
            .expect("reservation takeover");
        assert!(matches!(
            takeover,
            OutboundNotificationSmtpAttemptAdmission::Reserved(record)
                if record.fence_generation == 2 && record.fence_token == second_token
        ));
        assert!(matches!(
            repository
                .start_smtp_dispatch(
                    &delivery,
                    1,
                    first_token,
                    first_expiry + Duration::seconds(1),
                    first_expiry + Duration::seconds(11),
                )
                .await
                .expect("stale fence"),
            OutboundNotificationSmtpDispatchStart::Deferred { .. }
        ));

        let started_at = first_expiry + Duration::seconds(2);
        let deadline = started_at + Duration::seconds(10);
        assert!(matches!(
            repository
                .start_smtp_dispatch(&delivery, 1, second_token, started_at, deadline)
                .await
                .expect("start takeover"),
            OutboundNotificationSmtpDispatchStart::Authorized(_)
        ));
        assert!(matches!(
            repository
                .reserve_smtp_attempt(
                    &delivery,
                    1,
                    Uuid::now_v7(),
                    deadline - Duration::seconds(1),
                    deadline + Duration::seconds(29),
                )
                .await
                .expect("dispatch replay before deadline"),
            OutboundNotificationSmtpAttemptAdmission::Deferred { retry_not_before }
                if retry_not_before == deadline
        ));
        let recovered = repository
            .reserve_smtp_attempt(
                &delivery,
                1,
                Uuid::now_v7(),
                deadline,
                deadline + Duration::seconds(30),
            )
            .await
            .expect("dispatch replay at deadline");
        assert!(matches!(
            recovered,
            OutboundNotificationSmtpAttemptAdmission::Terminal(receipt)
                if receipt.outcome() == OutboundNotificationTerminalOutcome::Indeterminate
                    && receipt.terminal_at() == deadline
        ));
    }

    #[tokio::test]
    async fn definitive_authority_loss_obsoletes_before_the_dispatch_fence() {
        let (repository, delivery) = seeded(1).await;
        let reserved_at = delivery.occurred_at() + Duration::seconds(1);
        let token = Uuid::now_v7();
        repository
            .reserve_smtp_attempt(
                &delivery,
                1,
                token,
                reserved_at,
                reserved_at + Duration::seconds(30),
            )
            .await
            .expect("reservation");
        let settlement = repository
            .settle_smtp_attempt(
                &delivery,
                1,
                token,
                OutboundNotificationSmtpAttemptOutcome::Obsolete,
                reserved_at + Duration::seconds(1),
            )
            .await
            .expect("obsolete settlement");
        let receipt = settlement.receipt.expect("obsolete receipt");
        assert_eq!(
            receipt.outcome(),
            OutboundNotificationTerminalOutcome::Obsolete
        );
        assert!(settlement.attempt.dispatch_started_at.is_none());
    }
}
