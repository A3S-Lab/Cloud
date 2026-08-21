use super::{Notification, NotificationAlertSource, NotificationCursor};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, NotificationId, OrganizationId, PrincipalId,
    RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct MarkNotificationReadWrite {
    pub notification: Notification,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
    pub request_id: Uuid,
}

impl MarkNotificationReadWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.notification.validate()?;
        self.idempotency.validate()?;
        if self.actor_principal_id.as_uuid().is_nil()
            || self.request_id.is_nil()
            || self.notification.recipient_principal_id != self.actor_principal_id
            || self.expected_version.checked_add(1) != Some(self.notification.aggregate_version)
            || self.notification.read_at.is_none()
            || self.event.event_key != "notification.inbox.read"
            || self.event.schema_version != 1
            || self.event.organization_id != self.notification.organization_id.as_uuid()
            || self.event.aggregate_id != self.notification.id.as_uuid()
            || self.event.aggregate_version != self.notification.aggregate_version
            || self.event.occurred_at != self.notification.read_at.expect("validated read time")
            || self.event.correlation_id != self.request_id
            || self.event.causation_id != Some(self.notification.source_event_id)
        {
            return Err("notification read write is inconsistent".into());
        }
        Ok(())
    }

    pub fn validate_against(&self, existing: &Notification) -> Result<(), String> {
        self.validate()?;
        if existing.organization_id != self.notification.organization_id
            || existing.id != self.notification.id
            || existing.recipient_principal_id != self.notification.recipient_principal_id
            || existing.aggregate_version != self.expected_version
            || existing.read_at.is_some()
        {
            return Err("notification changed while marking it read".into());
        }
        let expected = existing.mark_read(
            self.expected_version,
            self.notification.read_at.expect("validated read time"),
        )?;
        if expected != self.notification {
            return Err("notification read transition is invalid".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait INotificationRepository: Send + Sync {
    /// Projects one committed outbox fact. Exact retries are successful no-ops.
    async fn project(&self, notification: Notification) -> Result<bool, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        notification_id: NotificationId,
    ) -> Result<Option<Notification>, RepositoryError>;

    /// Returns one raw recipient page ordered by source occurrence and notification ID descending.
    async fn list_page(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        unread_only: bool,
        after: Option<NotificationCursor>,
        limit: usize,
    ) -> Result<Vec<Notification>, RepositoryError>;

    /// Returns the latest already-projected fact in one closed alert source family before the
    /// supplied aggregate version. This is history, not a second mutable incident authority.
    async fn latest_alert_source_projection(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        source: NotificationAlertSource,
        source_aggregate_id: Uuid,
        not_before: DateTime<Utc>,
        before_aggregate_version: u64,
    ) -> Result<Option<Notification>, RepositoryError>;

    async fn replay_mark_read(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<Notification>>, RepositoryError>;

    async fn mark_read(
        &self,
        write: MarkNotificationReadWrite,
    ) -> Result<IdempotentWrite<Notification>, RepositoryError>;
}
