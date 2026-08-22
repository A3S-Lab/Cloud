use super::OutboundNotificationDispatchResult;
use crate::modules::identity::domain::repositories::{
    IRecipientContactRepository, ResolvedRecipientContact,
};
use crate::modules::notifications::domain::{
    IOutboundNotificationSmtpAttemptRepository, IOutboundNotificationSmtpDeliveryService,
    OutboundNotificationChannel, OutboundNotificationDelivery,
    OutboundNotificationSmtpAttemptAdmission, OutboundNotificationSmtpAttemptOutcome,
    OutboundNotificationSmtpAttemptSettlement, OutboundNotificationSmtpDispatchStart,
    OutboundNotificationSmtpPreparationError, OutboundNotificationSmtpProviderOutcome,
    MAXIMUM_OUTBOUND_NOTIFICATION_DELIVERY_GENERATION,
    MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_LEASE_SECONDS,
    MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_OUTCOME_SECONDS,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{canonical_timestamp, RepositoryError};
use chrono::{Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

pub struct OutboundNotificationSmtpDispatcher {
    attempts: Arc<dyn IOutboundNotificationSmtpAttemptRepository>,
    recipient_contacts: Arc<dyn IRecipientContactRepository>,
    delivery_service: Arc<dyn IOutboundNotificationSmtpDeliveryService>,
    reservation_lease: Duration,
    outcome_timeout: Duration,
}

impl OutboundNotificationSmtpDispatcher {
    pub fn new(
        attempts: Arc<dyn IOutboundNotificationSmtpAttemptRepository>,
        recipient_contacts: Arc<dyn IRecipientContactRepository>,
        delivery_service: Arc<dyn IOutboundNotificationSmtpDeliveryService>,
        reservation_lease: Duration,
        outcome_timeout: Duration,
    ) -> Result<Self, String> {
        if reservation_lease < Duration::seconds(30)
            || reservation_lease
                > Duration::seconds(MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_LEASE_SECONDS)
            || outcome_timeout <= Duration::zero()
            || outcome_timeout
                > Duration::seconds(MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_OUTCOME_SECONDS)
            || reservation_lease <= outcome_timeout
        {
            return Err("outbound SMTP notification timing policy is invalid".into());
        }
        Ok(Self {
            attempts,
            recipient_contacts,
            delivery_service,
            reservation_lease,
            outcome_timeout,
        })
    }

    pub async fn dispatch(
        &self,
        delivery: &OutboundNotificationDelivery,
        delivery_count: u64,
    ) -> ApplicationResult<OutboundNotificationDispatchResult> {
        delivery.validate().map_err(ApplicationError::Invalid)?;
        if delivery.channel() != OutboundNotificationChannel::Smtp
            || delivery.recipient_contact_id().is_none()
        {
            return Err(ApplicationError::Invalid(
                "SMTP dispatcher requires an exact recipient-contact delivery".into(),
            ));
        }
        if delivery_count == 0 {
            return Err(ApplicationError::Invalid(
                "A3S Event delivery count must be positive".into(),
            ));
        }

        let maximum_generation = delivery_count
            .min(MAXIMUM_OUTBOUND_NOTIFICATION_DELIVERY_GENERATION)
            .min(delivery.maximum_provider_attempts());
        for generation in 1..=maximum_generation {
            let reserved_at = canonical_timestamp(Utc::now());
            let fence_token = Uuid::now_v7();
            let reservation = self
                .attempts
                .reserve_smtp_attempt(
                    delivery,
                    generation,
                    fence_token,
                    reserved_at,
                    canonical_timestamp(reserved_at + self.reservation_lease),
                )
                .await
                .map_err(ApplicationError::from)?;
            let reservation = match reservation {
                OutboundNotificationSmtpAttemptAdmission::Reserved(record) => record,
                OutboundNotificationSmtpAttemptAdmission::Retryable(_record)
                    if generation < maximum_generation =>
                {
                    continue
                }
                OutboundNotificationSmtpAttemptAdmission::Retryable(record) => {
                    if generation == delivery.maximum_provider_attempts() {
                        return Err(ApplicationError::Internal(
                            "final SMTP retryable evidence has no Exhausted receipt".into(),
                        ));
                    }
                    return Ok(OutboundNotificationDispatchResult::SmtpRetryable {
                        generation,
                        attempt_id: record.attempt_id,
                    });
                }
                OutboundNotificationSmtpAttemptAdmission::Deferred { retry_not_before } => {
                    return Ok(OutboundNotificationDispatchResult::Deferred {
                        generation,
                        attempt_id: crate::modules::notifications::domain::outbound_notification_smtp_attempt_id(
                            delivery.id(),
                            generation,
                        )
                        .map_err(ApplicationError::Invalid)?,
                        retry_not_before,
                    })
                }
                OutboundNotificationSmtpAttemptAdmission::Terminal(receipt) => {
                    return self.terminal_result(delivery, receipt)
                }
                OutboundNotificationSmtpAttemptAdmission::InvalidFact => {
                    return Err(ApplicationError::Invalid(
                        "outbound SMTP notification delivery fact is not admitted".into(),
                    ))
                }
            };

            let first_resolution = match self.resolve_contact(delivery).await? {
                ContactResolution::Current(value) => value,
                ContactResolution::Obsolete => {
                    return self
                        .settle(
                            delivery,
                            generation,
                            reservation.fence_token,
                            OutboundNotificationSmtpAttemptOutcome::Obsolete,
                        )
                        .await
                }
            };
            let prepared = match self
                .delivery_service
                .prepare(delivery, first_resolution.address.clone())
                .await
            {
                Ok(value) => value,
                Err(OutboundNotificationSmtpPreparationError::Unavailable) => {
                    return Ok(OutboundNotificationDispatchResult::Deferred {
                        generation,
                        attempt_id: reservation.attempt_id,
                        retry_not_before: reservation.lease_expires_at,
                    })
                }
                Err(OutboundNotificationSmtpPreparationError::Invalid) => {
                    return Err(ApplicationError::Internal(
                        "outbound SMTP notification message preparation is invalid".into(),
                    ))
                }
            };

            let second_resolution = match self.resolve_contact(delivery).await? {
                ContactResolution::Current(value) => value,
                ContactResolution::Obsolete => {
                    return self
                        .settle(
                            delivery,
                            generation,
                            reservation.fence_token,
                            OutboundNotificationSmtpAttemptOutcome::Obsolete,
                        )
                        .await
                }
            };
            if second_resolution != first_resolution {
                return Err(ApplicationError::Internal(
                    "recipient contact identity changed during SMTP preparation".into(),
                ));
            }

            let started_at = canonical_timestamp(Utc::now());
            match self
                .attempts
                .start_smtp_dispatch(
                    delivery,
                    generation,
                    reservation.fence_token,
                    started_at,
                    canonical_timestamp(started_at + self.outcome_timeout),
                )
                .await
                .map_err(ApplicationError::from)?
            {
                OutboundNotificationSmtpDispatchStart::Authorized(_) => {}
                OutboundNotificationSmtpDispatchStart::Deferred { retry_not_before } => {
                    return Ok(OutboundNotificationDispatchResult::Deferred {
                        generation,
                        attempt_id: reservation.attempt_id,
                        retry_not_before,
                    })
                }
                OutboundNotificationSmtpDispatchStart::Terminal(receipt) => {
                    return self.terminal_result(delivery, receipt)
                }
            }

            let outcome = match prepared.deliver().await {
                OutboundNotificationSmtpProviderOutcome::Accepted => {
                    OutboundNotificationSmtpAttemptOutcome::Accepted
                }
                OutboundNotificationSmtpProviderOutcome::Rejected => {
                    OutboundNotificationSmtpAttemptOutcome::Rejected
                }
                OutboundNotificationSmtpProviderOutcome::Retryable => {
                    OutboundNotificationSmtpAttemptOutcome::Retryable
                }
                OutboundNotificationSmtpProviderOutcome::Indeterminate => {
                    OutboundNotificationSmtpAttemptOutcome::Indeterminate
                }
            };
            return self
                .settle(delivery, generation, reservation.fence_token, outcome)
                .await;
        }

        Err(ApplicationError::Internal(
            "outbound SMTP notification dispatch exhausted no generation".into(),
        ))
    }

    async fn resolve_contact(
        &self,
        delivery: &OutboundNotificationDelivery,
    ) -> ApplicationResult<ContactResolution> {
        let contact_id = delivery.recipient_contact_id().ok_or_else(|| {
            ApplicationError::Invalid("SMTP delivery has no recipient contact".into())
        })?;
        match self
            .recipient_contacts
            .resolve_verified_recipient_contact(
                delivery.organization_id(),
                delivery.recipient_principal_id(),
                contact_id,
            )
            .await
        {
            Ok(Some(contact))
                if contact.id == contact_id
                    && contact.principal_id == delivery.recipient_principal_id() =>
            {
                Ok(ContactResolution::Current(contact))
            }
            Ok(Some(_)) => Err(ApplicationError::Internal(
                "recipient contact resolver returned inconsistent authority".into(),
            )),
            Ok(None) | Err(RepositoryError::NotFound) => Ok(ContactResolution::Obsolete),
            Err(error) => Err(error.into()),
        }
    }

    async fn settle(
        &self,
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        fence_token: Uuid,
        outcome: OutboundNotificationSmtpAttemptOutcome,
    ) -> ApplicationResult<OutboundNotificationDispatchResult> {
        let settlement = self
            .attempts
            .settle_smtp_attempt(
                delivery,
                generation,
                fence_token,
                outcome,
                canonical_timestamp(Utc::now()),
            )
            .await
            .map_err(ApplicationError::from)?;
        self.settlement_result(delivery, settlement)
    }

    fn settlement_result(
        &self,
        delivery: &OutboundNotificationDelivery,
        settlement: OutboundNotificationSmtpAttemptSettlement,
    ) -> ApplicationResult<OutboundNotificationDispatchResult> {
        if let Some(receipt) = settlement.receipt {
            return self.terminal_result(delivery, receipt);
        }
        if settlement.attempt.outcome != Some(OutboundNotificationSmtpAttemptOutcome::Retryable)
            || settlement.attempt.generation == delivery.maximum_provider_attempts()
        {
            return Err(ApplicationError::Internal(
                "SMTP attempt settlement omitted its required terminal receipt".into(),
            ));
        }
        Ok(OutboundNotificationDispatchResult::SmtpRetryable {
            generation: settlement.attempt.generation,
            attempt_id: settlement.attempt.attempt_id,
        })
    }

    fn terminal_result(
        &self,
        delivery: &OutboundNotificationDelivery,
        receipt: crate::modules::notifications::domain::OutboundNotificationTerminalReceipt,
    ) -> ApplicationResult<OutboundNotificationDispatchResult> {
        receipt
            .validate_against(delivery)
            .map_err(ApplicationError::Internal)?;
        Ok(OutboundNotificationDispatchResult::TerminalPersisted { receipt })
    }
}

enum ContactResolution {
    Current(ResolvedRecipientContact),
    Obsolete,
}

#[cfg(test)]
#[path = "outbound_smtp_dispatch_tests.rs"]
mod tests;
