use super::{
    outbound_notification_attempt_id, OutboundNotificationConnectorTarget,
    OutboundNotificationDelivery, MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
};
use crate::modules::connectors::{ConnectorExecutionEvidence, ConnectorExecutionOutcome};
use crate::modules::shared_kernel::domain::{canonical_timestamp, OrganizationId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboundNotificationTerminalOutcome {
    Delivered,
    Rejected,
    Indeterminate,
    Exhausted,
}

impl OutboundNotificationTerminalOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::Rejected => "rejected",
            Self::Indeterminate => "indeterminate",
            Self::Exhausted => "exhausted",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "delivered" => Ok(Self::Delivered),
            "rejected" => Ok(Self::Rejected),
            "indeterminate" => Ok(Self::Indeterminate),
            "exhausted" => Ok(Self::Exhausted),
            _ => Err("outbound notification terminal outcome is unsupported".into()),
        }
    }
}

/// Notification-owned logical terminal acknowledgement for one delivery.
///
/// The receipt references C6's exact attempt but does not copy provider bodies,
/// credentials, response text, retry state, or Connector evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundNotificationTerminalReceipt {
    organization_id: OrganizationId,
    delivery_id: Uuid,
    target: OutboundNotificationConnectorTarget,
    outcome: OutboundNotificationTerminalOutcome,
    generation: u64,
    attempt_id: Uuid,
    terminal_at: DateTime<Utc>,
}

impl OutboundNotificationTerminalReceipt {
    pub fn delivered(
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        evidence: &ConnectorExecutionEvidence,
    ) -> Result<Self, String> {
        Self::from_evidence(
            delivery,
            generation,
            evidence,
            ConnectorExecutionOutcome::Accepted,
            OutboundNotificationTerminalOutcome::Delivered,
        )
    }

    pub fn rejected(
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        evidence: &ConnectorExecutionEvidence,
    ) -> Result<Self, String> {
        Self::from_evidence(
            delivery,
            generation,
            evidence,
            ConnectorExecutionOutcome::Rejected,
            OutboundNotificationTerminalOutcome::Rejected,
        )
    }

    pub fn exhausted(
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        evidence: &ConnectorExecutionEvidence,
    ) -> Result<Self, String> {
        Self::from_evidence(
            delivery,
            generation,
            evidence,
            ConnectorExecutionOutcome::Retryable,
            OutboundNotificationTerminalOutcome::Exhausted,
        )
    }

    pub fn indeterminate(
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        attempt_id: Uuid,
        outcome_deadline_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::restore(
            delivery.organization_id(),
            delivery.id(),
            delivery.target(),
            OutboundNotificationTerminalOutcome::Indeterminate,
            generation,
            attempt_id,
            outcome_deadline_at,
        )
        .and_then(|receipt| {
            receipt.validate_against(delivery)?;
            Ok(receipt)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        delivery_id: Uuid,
        target: OutboundNotificationConnectorTarget,
        outcome: OutboundNotificationTerminalOutcome,
        generation: u64,
        attempt_id: Uuid,
        terminal_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let receipt = Self {
            organization_id,
            delivery_id,
            target,
            outcome,
            generation,
            attempt_id,
            terminal_at: canonical_timestamp(terminal_at),
        };
        receipt.validate()?;
        Ok(receipt)
    }

    fn from_evidence(
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        evidence: &ConnectorExecutionEvidence,
        expected_connector_outcome: ConnectorExecutionOutcome,
        outcome: OutboundNotificationTerminalOutcome,
    ) -> Result<Self, String> {
        evidence.validate()?;
        let target = delivery.target();
        if evidence.organization_id() != delivery.organization_id()
            || evidence.project_id() != target.project_id
            || evidence.environment_id() != target.environment_id
            || evidence.profile_id() != target.profile_id
            || evidence.revision_id() != target.revision_id
            || evidence.outcome() != expected_connector_outcome
        {
            return Err(
                "Connector evidence does not match the outbound notification delivery".into(),
            );
        }
        let receipt = Self::restore(
            delivery.organization_id(),
            delivery.id(),
            target,
            outcome,
            generation,
            evidence.attempt_id(),
            evidence.completed_at(),
        )?;
        receipt.validate_against(delivery)?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.target.validate()?;
        if self.organization_id.as_uuid().is_nil()
            || self.delivery_id.is_nil()
            || self.attempt_id.is_nil()
            || self.attempt_id
                != outbound_notification_attempt_id(self.delivery_id, self.generation)?
            || self.outcome == OutboundNotificationTerminalOutcome::Exhausted
                && self.generation != MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS
            || self.terminal_at != canonical_timestamp(self.terminal_at)
        {
            return Err("outbound notification terminal receipt is invalid".into());
        }
        Ok(())
    }

    pub fn validate_against(&self, delivery: &OutboundNotificationDelivery) -> Result<(), String> {
        self.validate()?;
        delivery.validate()?;
        if self.organization_id != delivery.organization_id()
            || self.delivery_id != delivery.id()
            || self.target != delivery.target()
            || self.terminal_at < delivery.occurred_at()
        {
            return Err(
                "outbound notification terminal receipt does not match its delivery".into(),
            );
        }
        Ok(())
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn delivery_id(&self) -> Uuid {
        self.delivery_id
    }

    pub const fn target(&self) -> OutboundNotificationConnectorTarget {
        self.target
    }

    pub const fn outcome(&self) -> OutboundNotificationTerminalOutcome {
        self.outcome
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn attempt_id(&self) -> Uuid {
        self.attempt_id
    }

    pub const fn terminal_at(&self) -> DateTime<Utc> {
        self.terminal_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::notifications::{
        Notification, NotificationScope, NotificationSeverity, OutboundNotificationChannel,
    };
    use crate::modules::shared_kernel::domain::{
        ConnectorProfileId, ConnectorRevisionId, EnvironmentId, PrincipalId, ProjectId,
    };

    fn delivery() -> OutboundNotificationDelivery {
        let now = canonical_timestamp(Utc::now());
        let notification = Notification::project(
            OrganizationId::new(),
            PrincipalId::new(),
            Uuid::now_v7(),
            "identity.membership.role-changed".into(),
            1,
            Uuid::now_v7(),
            2,
            Uuid::now_v7(),
            NotificationSeverity::Warning,
            "Organization role changed".into(),
            "Your organization role is now member.".into(),
            NotificationScope::Organization,
            now,
            now,
        )
        .expect("notification");
        OutboundNotificationDelivery::from_notification(
            &notification,
            OutboundNotificationChannel::SlackCompatible,
            OutboundNotificationConnectorTarget::new(
                ProjectId::new(),
                EnvironmentId::new(),
                ConnectorProfileId::new(),
                ConnectorRevisionId::new(),
            )
            .expect("target"),
        )
        .expect("delivery")
    }

    #[test]
    fn indeterminate_receipt_is_exact_and_generation_fenced() {
        let delivery = delivery();
        let attempt_id = outbound_notification_attempt_id(delivery.id(), 3).expect("attempt");
        let receipt = OutboundNotificationTerminalReceipt::indeterminate(
            &delivery,
            3,
            attempt_id,
            delivery.occurred_at() + chrono::Duration::seconds(30),
        )
        .expect("receipt");
        assert_eq!(
            receipt.outcome(),
            OutboundNotificationTerminalOutcome::Indeterminate
        );
        assert!(OutboundNotificationTerminalReceipt::indeterminate(
            &delivery,
            4,
            attempt_id,
            receipt.terminal_at(),
        )
        .is_err());
    }
}
