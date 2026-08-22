use super::{
    OutboundNotificationDelivery, OutboundNotificationSmtpAttemptOutcome,
    OutboundNotificationSmtpAttemptRecord, OutboundNotificationTerminalReceipt,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundNotificationSmtpAttemptAdmission {
    Reserved(OutboundNotificationSmtpAttemptRecord),
    Retryable(OutboundNotificationSmtpAttemptRecord),
    Deferred { retry_not_before: DateTime<Utc> },
    Terminal(OutboundNotificationTerminalReceipt),
    InvalidFact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundNotificationSmtpDispatchStart {
    Authorized(OutboundNotificationSmtpAttemptRecord),
    Deferred { retry_not_before: DateTime<Utc> },
    Terminal(OutboundNotificationTerminalReceipt),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundNotificationSmtpAttemptSettlement {
    pub attempt: OutboundNotificationSmtpAttemptRecord,
    pub receipt: Option<OutboundNotificationTerminalReceipt>,
}

#[async_trait]
pub trait IOutboundNotificationSmtpAttemptRepository: Send + Sync {
    async fn reserve_smtp_attempt(
        &self,
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        fence_token: Uuid,
        reserved_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<OutboundNotificationSmtpAttemptAdmission, RepositoryError>;

    async fn start_smtp_dispatch(
        &self,
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        fence_token: Uuid,
        started_at: DateTime<Utc>,
        outcome_deadline_at: DateTime<Utc>,
    ) -> Result<OutboundNotificationSmtpDispatchStart, RepositoryError>;

    async fn settle_smtp_attempt(
        &self,
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        fence_token: Uuid,
        outcome: OutboundNotificationSmtpAttemptOutcome,
        settled_at: DateTime<Utc>,
    ) -> Result<OutboundNotificationSmtpAttemptSettlement, RepositoryError>;

    async fn find_smtp_attempt(
        &self,
        organization_id: crate::modules::shared_kernel::domain::OrganizationId,
        delivery_id: Uuid,
        generation: u64,
    ) -> Result<Option<OutboundNotificationSmtpAttemptRecord>, RepositoryError>;
}
