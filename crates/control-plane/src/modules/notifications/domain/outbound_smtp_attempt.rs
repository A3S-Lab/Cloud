use super::{
    OutboundNotificationTerminalOutcome, MAXIMUM_OUTBOUND_NOTIFICATION_DELIVERY_GENERATION,
    MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, RecipientContactId,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_LEASE_SECONDS: i64 = 300;
pub const MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_OUTCOME_SECONDS: i64 = 120;

pub fn outbound_notification_smtp_attempt_id(
    delivery_id: Uuid,
    generation: u64,
) -> Result<Uuid, String> {
    if delivery_id.is_nil()
        || generation == 0
        || generation > MAXIMUM_OUTBOUND_NOTIFICATION_DELIVERY_GENERATION
    {
        return Err("outbound SMTP notification attempt generation is invalid".into());
    }
    Ok(Uuid::new_v5(
        &delivery_id,
        format!("smtp-attempt:{generation}").as_bytes(),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboundNotificationSmtpAttemptState {
    Reserved,
    Dispatching,
    Terminal,
}

impl OutboundNotificationSmtpAttemptState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::Dispatching => "dispatching",
            Self::Terminal => "terminal",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "reserved" => Ok(Self::Reserved),
            "dispatching" => Ok(Self::Dispatching),
            "terminal" => Ok(Self::Terminal),
            _ => Err("outbound SMTP notification attempt state is invalid".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboundNotificationSmtpAttemptOutcome {
    Accepted,
    Rejected,
    Retryable,
    Indeterminate,
    Obsolete,
}

impl OutboundNotificationSmtpAttemptOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Retryable => "retryable",
            Self::Indeterminate => "indeterminate",
            Self::Obsolete => "obsolete",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            "retryable" => Ok(Self::Retryable),
            "indeterminate" => Ok(Self::Indeterminate),
            "obsolete" => Ok(Self::Obsolete),
            _ => Err("outbound SMTP notification attempt outcome is invalid".into()),
        }
    }

    pub const fn terminal_receipt_outcome(
        self,
        generation: u64,
        maximum_provider_attempts: u64,
    ) -> Option<OutboundNotificationTerminalOutcome> {
        match self {
            Self::Accepted => Some(OutboundNotificationTerminalOutcome::Delivered),
            Self::Rejected => Some(OutboundNotificationTerminalOutcome::Rejected),
            Self::Retryable if generation == maximum_provider_attempts => {
                Some(OutboundNotificationTerminalOutcome::Exhausted)
            }
            Self::Retryable => None,
            Self::Indeterminate => Some(OutboundNotificationTerminalOutcome::Indeterminate),
            Self::Obsolete => Some(OutboundNotificationTerminalOutcome::Obsolete),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundNotificationSmtpAttemptRecord {
    pub organization_id: OrganizationId,
    pub delivery_id: Uuid,
    pub recipient_contact_id: RecipientContactId,
    pub generation: u64,
    pub attempt_id: Uuid,
    pub state: OutboundNotificationSmtpAttemptState,
    pub outcome: Option<OutboundNotificationSmtpAttemptOutcome>,
    pub fence_generation: u64,
    pub fence_token: Uuid,
    pub reserved_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
    pub dispatch_started_at: Option<DateTime<Utc>>,
    pub outcome_deadline_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl OutboundNotificationSmtpAttemptRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        delivery_id: Uuid,
        recipient_contact_id: RecipientContactId,
        generation: u64,
        attempt_id: Uuid,
        state: OutboundNotificationSmtpAttemptState,
        outcome: Option<OutboundNotificationSmtpAttemptOutcome>,
        fence_generation: u64,
        fence_token: Uuid,
        reserved_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
        dispatch_started_at: Option<DateTime<Utc>>,
        outcome_deadline_at: Option<DateTime<Utc>>,
        completed_at: Option<DateTime<Utc>>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            delivery_id,
            recipient_contact_id,
            generation,
            attempt_id,
            state,
            outcome,
            fence_generation,
            fence_token,
            reserved_at,
            lease_expires_at,
            dispatch_started_at,
            outcome_deadline_at,
            completed_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        let lease_duration = self.lease_expires_at - self.reserved_at;
        let common_valid = !self.organization_id.as_uuid().is_nil()
            && !self.delivery_id.is_nil()
            && !self.recipient_contact_id.as_uuid().is_nil()
            && self.generation > 0
            && self.generation <= MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS
            && self.attempt_id
                == outbound_notification_smtp_attempt_id(self.delivery_id, self.generation)?
            && self.fence_generation > 0
            && !self.fence_token.is_nil()
            && self.reserved_at == canonical_timestamp(self.reserved_at)
            && self.lease_expires_at == canonical_timestamp(self.lease_expires_at)
            && lease_duration > Duration::zero()
            && lease_duration
                <= Duration::seconds(MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_LEASE_SECONDS)
            && self.dispatch_started_at == self.dispatch_started_at.map(canonical_timestamp)
            && self.outcome_deadline_at == self.outcome_deadline_at.map(canonical_timestamp)
            && self.completed_at == self.completed_at.map(canonical_timestamp);
        let state_valid = match (self.state, self.outcome) {
            (OutboundNotificationSmtpAttemptState::Reserved, None) => {
                self.dispatch_started_at.is_none()
                    && self.outcome_deadline_at.is_none()
                    && self.completed_at.is_none()
            }
            (OutboundNotificationSmtpAttemptState::Dispatching, None) => self
                .dispatch_window()
                .is_some_and(|(_, _)| self.completed_at.is_none()),
            (
                OutboundNotificationSmtpAttemptState::Terminal,
                Some(OutboundNotificationSmtpAttemptOutcome::Obsolete),
            ) => {
                self.dispatch_started_at.is_none()
                    && self.outcome_deadline_at.is_none()
                    && self.completed_at.is_some_and(|completed| {
                        completed >= self.reserved_at && completed <= self.lease_expires_at
                    })
            }
            (OutboundNotificationSmtpAttemptState::Terminal, Some(_)) => self
                .dispatch_window()
                .zip(self.completed_at)
                .is_some_and(|((started, _), completed)| completed >= started),
            _ => false,
        };
        if !common_valid || !state_valid {
            return Err("outbound SMTP notification attempt record is invalid".into());
        }
        Ok(())
    }

    fn dispatch_window(&self) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        self.dispatch_started_at
            .zip(self.outcome_deadline_at)
            .filter(|(started, deadline)| {
                *started >= self.reserved_at
                    && *started < self.lease_expires_at
                    && *deadline > *started
                    && *deadline
                        <= *started
                            + Duration::seconds(MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_OUTCOME_SECONDS)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smtp_attempt_identity_is_distinct_and_state_is_closed() {
        let delivery_id = Uuid::now_v7();
        let smtp = outbound_notification_smtp_attempt_id(delivery_id, 1).expect("SMTP attempt");
        let connector = Uuid::new_v5(&delivery_id, b"connector-attempt:1");
        assert_ne!(smtp, connector);
        assert!(outbound_notification_smtp_attempt_id(delivery_id, 0).is_err());

        let now = canonical_timestamp(Utc::now());
        let record = OutboundNotificationSmtpAttemptRecord::restore(
            OrganizationId::new(),
            delivery_id,
            RecipientContactId::new(),
            1,
            smtp,
            OutboundNotificationSmtpAttemptState::Reserved,
            None,
            1,
            Uuid::now_v7(),
            now,
            now + Duration::seconds(30),
            None,
            None,
            None,
        )
        .expect("reservation");
        assert_eq!(record.state, OutboundNotificationSmtpAttemptState::Reserved);
    }

    #[test]
    fn only_exact_budget_retryable_becomes_exhausted() {
        assert_eq!(
            OutboundNotificationSmtpAttemptOutcome::Retryable.terminal_receipt_outcome(2, 3),
            None
        );
        assert_eq!(
            OutboundNotificationSmtpAttemptOutcome::Retryable.terminal_receipt_outcome(3, 3),
            Some(OutboundNotificationTerminalOutcome::Exhausted)
        );
    }
}
