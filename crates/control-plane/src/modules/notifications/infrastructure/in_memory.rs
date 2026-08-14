use crate::modules::notifications::domain::{
    INotificationRepository, MarkNotificationReadWrite, Notification, NotificationCursor,
};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, NotificationId, OrganizationId, PrincipalId,
    RepositoryError,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryNotificationRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    notifications: BTreeMap<(OrganizationId, NotificationId), Notification>,
    source_events: BTreeMap<(uuid::Uuid, PrincipalId), NotificationId>,
    read_idempotency: BTreeMap<(String, String), (String, Notification)>,
    outbox: Vec<a3s_cloud_contracts::DomainEventEnvelope>,
}

impl InMemoryNotificationRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<a3s_cloud_contracts::DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl INotificationRepository for InMemoryNotificationRepository {
    async fn project(&self, notification: Notification) -> Result<bool, RepositoryError> {
        notification.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let source_key = (
            notification.source_event_id,
            notification.recipient_principal_id,
        );
        if let Some(id) = state.source_events.get(&source_key) {
            let existing = state
                .notifications
                .get(&(notification.organization_id, *id))
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "notification source index points to a missing record".into(),
                    )
                })?;
            if existing != &notification {
                return Err(RepositoryError::Conflict(
                    "notification source event replay changed its immutable projection".into(),
                ));
            }
            return Ok(false);
        }
        let key = (notification.organization_id, notification.id);
        if state.notifications.contains_key(&key) {
            return Err(RepositoryError::Conflict(
                "notification ID is already in use".into(),
            ));
        }
        state.source_events.insert(source_key, notification.id);
        state.notifications.insert(key, notification);
        Ok(true)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        notification_id: NotificationId,
    ) -> Result<Option<Notification>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .notifications
            .get(&(organization_id, notification_id))
            .filter(|notification| notification.recipient_principal_id == recipient_principal_id)
            .cloned())
    }

    async fn list_page(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        unread_only: bool,
        after: Option<NotificationCursor>,
        limit: usize,
    ) -> Result<Vec<Notification>, RepositoryError> {
        let mut notifications = self
            .state
            .read()
            .await
            .notifications
            .values()
            .filter(|notification| {
                notification.organization_id == organization_id
                    && notification.recipient_principal_id == recipient_principal_id
                    && (!unread_only || notification.read_at.is_none())
            })
            .filter(|notification| {
                after.is_none_or(|cursor| {
                    notification.occurred_at < cursor.occurred_at
                        || (notification.occurred_at == cursor.occurred_at
                            && notification.id < cursor.notification_id)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        notifications.sort_by(|left, right| {
            right
                .occurred_at
                .cmp(&left.occurred_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        notifications.truncate(limit.max(1));
        Ok(notifications)
    }

    async fn replay_mark_read(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<Notification>>, RepositoryError> {
        idempotency.validate().map_err(RepositoryError::Storage)?;
        let state = self.state.read().await;
        let key = (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        );
        match state.read_idempotency.get(&key) {
            Some((digest, _)) if digest != &idempotency.request_digest => {
                Err(RepositoryError::IdempotencyConflict)
            }
            Some((_, notification)) => Ok(Some(IdempotentWrite {
                value: notification.clone(),
                replayed: true,
            })),
            None => Ok(None),
        }
    }

    async fn mark_read(
        &self,
        write: MarkNotificationReadWrite,
    ) -> Result<IdempotentWrite<Notification>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let idempotency_key = (
            write.idempotency.storage_key().0.to_owned(),
            write.idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, notification)) = state.read_idempotency.get(&idempotency_key) {
            if digest != &write.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: notification.clone(),
                replayed: true,
            });
        }
        let key = (write.notification.organization_id, write.notification.id);
        let existing = state
            .notifications
            .get(&key)
            .ok_or(RepositoryError::NotFound)?;
        write.validate_against(existing).map_err(|_| {
            RepositoryError::Conflict("notification changed while marking it read".into())
        })?;
        state.notifications.insert(key, write.notification.clone());
        state.read_idempotency.insert(
            idempotency_key,
            (write.idempotency.request_digest, write.notification.clone()),
        );
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.notification,
            replayed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::notifications::domain::{NotificationScope, NotificationSeverity};
    use chrono::Utc;
    use uuid::Uuid;

    fn projected() -> Notification {
        let now = Utc::now();
        Notification::project(
            OrganizationId::new(),
            PrincipalId::new(),
            Uuid::now_v7(),
            "identity.membership.created".into(),
            1,
            Uuid::now_v7(),
            1,
            Uuid::now_v7(),
            NotificationSeverity::Information,
            "Organization access granted".into(),
            "You can now access this organization as member.".into(),
            NotificationScope::Organization,
            now,
            now,
        )
        .expect("notification")
    }

    #[tokio::test]
    async fn exact_projection_retries_are_noops_but_drift_is_rejected() {
        let repository = InMemoryNotificationRepository::new();
        let notification = projected();
        assert!(repository
            .project(notification.clone())
            .await
            .expect("first"));
        assert!(!repository
            .project(notification.clone())
            .await
            .expect("replay"));
        let mut drift = notification;
        drift.title = "Changed title".into();
        assert!(matches!(
            repository.project(drift).await,
            Err(RepositoryError::Conflict(_))
        ));
    }
}
