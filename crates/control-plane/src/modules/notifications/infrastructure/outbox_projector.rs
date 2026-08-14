use crate::modules::identity::domain::events::{MembershipChanged, MembershipInvitationChanged};
use crate::modules::identity::domain::repositories::IMembershipRepository;
use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::integration_events::{IIntegrationEventProjector, OutboxMessage};
use crate::modules::notifications::domain::{
    INotificationRepository, Notification, NotificationScope, NotificationSeverity,
};
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, RepositoryError};
use async_trait::async_trait;
use std::sync::Arc;

pub struct OutboxNotificationProjector {
    notifications: Arc<dyn INotificationRepository>,
    memberships: Arc<dyn IMembershipRepository>,
}

impl OutboxNotificationProjector {
    pub fn new(
        notifications: Arc<dyn INotificationRepository>,
        memberships: Arc<dyn IMembershipRepository>,
    ) -> Self {
        Self {
            notifications,
            memberships,
        }
    }

    async fn notification_for(
        &self,
        message: &OutboxMessage,
    ) -> Result<Option<Notification>, RepositoryError> {
        if message.schema_version != 1 {
            return Ok(None);
        }
        let (recipient, severity, title, body) = match message.event_key.as_str() {
            "identity.membership-invitation.created" => {
                let payload = decode_invitation(message)?;
                (
                    PrincipalId::from_uuid(payload.principal_id),
                    NotificationSeverity::Information,
                    "Organization invitation created".to_owned(),
                    format!(
                        "You were invited to join this organization as {}.",
                        payload.role
                    ),
                )
            }
            "identity.membership-invitation.revoked" => {
                let payload = decode_invitation(message)?;
                (
                    PrincipalId::from_uuid(payload.principal_id),
                    NotificationSeverity::Warning,
                    "Organization invitation revoked".to_owned(),
                    "Your pending invitation to this organization was revoked.".to_owned(),
                )
            }
            "identity.membership.created" => {
                let payload = decode_membership(message)?;
                (
                    PrincipalId::from_uuid(payload.principal_id),
                    NotificationSeverity::Information,
                    "Organization access granted".to_owned(),
                    format!("You can now access this organization as {}.", payload.role),
                )
            }
            "identity.membership.role-changed" => {
                let payload = decode_membership(message)?;
                (
                    PrincipalId::from_uuid(payload.principal_id),
                    NotificationSeverity::Information,
                    "Organization role changed".to_owned(),
                    format!("Your organization role is now {}.", payload.role),
                )
            }
            "identity.membership.revoked" => {
                let payload = decode_membership(message)?;
                (
                    PrincipalId::from_uuid(payload.principal_id),
                    NotificationSeverity::Critical,
                    "Organization access revoked".to_owned(),
                    "Your access to this organization was revoked.".to_owned(),
                )
            }
            _ => return Ok(None),
        };

        // Fail closed if the event payload claims a principal that does not match the owning
        // Membership aggregate. Invitations intentionally precede membership creation.
        if message.event_key.starts_with("identity.membership.") {
            let membership = self
                .memberships
                .find_membership(
                    OrganizationId::from_uuid(message.organization_id),
                    crate::modules::shared_kernel::domain::MembershipId::from_uuid(
                        message.aggregate_id,
                    ),
                )
                .await?
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "notification source membership no longer exists".into(),
                    )
                })?;
            if membership.membership.principal_id != recipient {
                return Err(RepositoryError::Storage(
                    "notification source membership principal is inconsistent".into(),
                ));
            }
        }

        Notification::project(
            OrganizationId::from_uuid(message.organization_id),
            recipient,
            message.event_id,
            message.event_key.clone(),
            message.schema_version,
            message.aggregate_id,
            message.aggregate_version,
            message.correlation_id,
            severity,
            title,
            body,
            NotificationScope::Organization,
            message.occurred_at,
            // This is the logical in-app delivery time. Deriving it from the immutable source
            // fact makes a relay retry an exact projection replay.
            message.occurred_at,
        )
        .map(Some)
        .map_err(RepositoryError::Storage)
    }
}

#[async_trait]
impl IIntegrationEventProjector for OutboxNotificationProjector {
    async fn project(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        if let Some(notification) = self.notification_for(message).await? {
            self.notifications.project(notification).await?;
        }
        Ok(())
    }
}

fn decode_membership(message: &OutboxMessage) -> Result<MembershipChanged, RepositoryError> {
    let payload: MembershipChanged =
        serde_json::from_value(message.payload.clone()).map_err(|error| {
            RepositoryError::Storage(format!(
                "notification source membership payload is invalid: {error}"
            ))
        })?;
    validate_identity_payload(
        message,
        payload.membership_id,
        payload.principal_id,
        &payload.role,
        "membership",
    )?;
    Ok(payload)
}

fn decode_invitation(
    message: &OutboxMessage,
) -> Result<MembershipInvitationChanged, RepositoryError> {
    let payload: MembershipInvitationChanged = serde_json::from_value(message.payload.clone())
        .map_err(|error| {
            RepositoryError::Storage(format!(
                "notification source invitation payload is invalid: {error}"
            ))
        })?;
    validate_identity_payload(
        message,
        payload.invitation_id,
        payload.principal_id,
        &payload.role,
        "invitation",
    )?;
    Ok(payload)
}

fn validate_identity_payload(
    message: &OutboxMessage,
    aggregate_id: uuid::Uuid,
    principal_id: uuid::Uuid,
    role: &str,
    label: &str,
) -> Result<(), RepositoryError> {
    if aggregate_id.is_nil()
        || aggregate_id != message.aggregate_id
        || principal_id.is_nil()
        || MembershipRole::parse(role).is_err()
    {
        return Err(RepositoryError::Storage(format!(
            "notification source {label} payload identity is inconsistent"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::InMemoryIdentityRepository;
    use crate::modules::integration_events::{
        EventPublishError, IEventPublisher, IOutboxRepository, OutboxRelay, OutboxRelayConfig,
    };
    use crate::modules::notifications::InMemoryNotificationRepository;
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    struct RetryingOutboxRepository {
        state: Mutex<RetryingOutboxState>,
    }

    struct RetryingOutboxState {
        message: OutboxMessage,
        published: bool,
        failures: usize,
    }

    impl RetryingOutboxRepository {
        fn new(message: OutboxMessage) -> Self {
            Self {
                state: Mutex::new(RetryingOutboxState {
                    message,
                    published: false,
                    failures: 0,
                }),
            }
        }
    }

    #[async_trait]
    impl IOutboxRepository for RetryingOutboxRepository {
        async fn claim(
            &self,
            _owner: Uuid,
            _limit: usize,
            _lease_duration: Duration,
        ) -> Result<Vec<OutboxMessage>, RepositoryError> {
            let state = self.state.lock().await;
            Ok((!state.published)
                .then(|| vec![state.message.clone()])
                .unwrap_or_default())
        }

        async fn mark_published(
            &self,
            event_id: Uuid,
            _owner: Uuid,
            _published_at: DateTime<Utc>,
        ) -> Result<(), RepositoryError> {
            let mut state = self.state.lock().await;
            if state.message.event_id != event_id {
                return Err(RepositoryError::NotFound);
            }
            state.published = true;
            Ok(())
        }

        async fn mark_failed(
            &self,
            event_id: Uuid,
            _owner: Uuid,
            _error: &str,
            _retry_after: Duration,
        ) -> Result<(), RepositoryError> {
            let mut state = self.state.lock().await;
            if state.message.event_id != event_id {
                return Err(RepositoryError::NotFound);
            }
            state.failures += 1;
            state.message.delivery_attempts += 1;
            Ok(())
        }
    }

    #[derive(Default)]
    struct FailOncePublisher {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl IEventPublisher for FailOncePublisher {
        async fn publish(&self, _message: &OutboxMessage) -> Result<(), EventPublishError> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(EventPublishError::new("provider unavailable"))
            } else {
                Ok(())
            }
        }

        async fn health(&self) -> Result<bool, EventPublishError> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn provider_retry_replays_one_logical_notification() {
        let organization_id = OrganizationId::new();
        let recipient = PrincipalId::new();
        let invitation_id = Uuid::now_v7();
        let message = OutboxMessage {
            event_id: Uuid::now_v7(),
            event_key: "identity.membership-invitation.created".into(),
            schema_version: 1,
            organization_id: organization_id.as_uuid(),
            aggregate_id: invitation_id,
            aggregate_version: 1,
            occurred_at: Utc::now(),
            correlation_id: Uuid::now_v7(),
            causation_id: None,
            payload: serde_json::json!({
                "invitation_id": invitation_id,
                "principal_id": recipient,
                "role": "member",
                "accepted_membership_id": null
            }),
            delivery_attempts: 1,
        };
        let outbox = Arc::new(RetryingOutboxRepository::new(message));
        let publisher = Arc::new(FailOncePublisher::default());
        let notifications = Arc::new(InMemoryNotificationRepository::new());
        let projector = Arc::new(OutboxNotificationProjector::new(
            notifications.clone(),
            Arc::new(InMemoryIdentityRepository::new()),
        ));
        let relay = OutboxRelay::new(
            outbox.clone(),
            publisher.clone(),
            OutboxRelayConfig {
                batch_size: 10,
                poll_interval: Duration::from_millis(10),
                lease_duration: Duration::from_secs(2),
                publish_timeout: Duration::from_secs(1),
                initial_backoff: Duration::from_millis(10),
                maximum_backoff: Duration::from_secs(1),
            },
        )
        .expect("relay")
        .with_projector(projector);

        let first = relay.run_once().await.expect("first relay attempt");
        assert_eq!(first.claimed, 1);
        assert_eq!(first.published, 0);
        assert_eq!(first.failures.len(), 1);
        assert_eq!(
            notifications
                .list_page(organization_id, recipient, false, None, 50)
                .await
                .expect("projected notifications")
                .len(),
            1
        );

        let retry = relay.run_once().await.expect("relay retry");
        assert_eq!(retry.claimed, 1);
        assert_eq!(retry.published, 1);
        assert!(retry.failures.is_empty());
        assert_eq!(publisher.calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            notifications
                .list_page(organization_id, recipient, false, None, 50)
                .await
                .expect("deduplicated notifications")
                .len(),
            1
        );
        assert_eq!(relay.run_once().await.expect("settled relay").claimed, 0);
        let state = outbox.state.lock().await;
        assert_eq!(state.failures, 1);
        assert!(state.published);
    }

    #[test]
    fn malformed_identity_payload_fails_closed() {
        let aggregate_id = Uuid::now_v7();
        let message = OutboxMessage {
            event_id: Uuid::now_v7(),
            event_key: "identity.membership-invitation.created".into(),
            schema_version: 1,
            organization_id: Uuid::now_v7(),
            aggregate_id,
            aggregate_version: 1,
            occurred_at: Utc::now(),
            correlation_id: Uuid::now_v7(),
            causation_id: None,
            payload: serde_json::json!({
                "invitation_id": Uuid::now_v7(),
                "principal_id": Uuid::now_v7(),
                "role": "member",
                "accepted_membership_id": null
            }),
            delivery_attempts: 1,
        };
        assert!(decode_invitation(&message).is_err());

        let mut invalid_role = message;
        invalid_role.payload["invitation_id"] = serde_json::json!(aggregate_id);
        invalid_role.payload["role"] = serde_json::json!("super-admin");
        assert!(decode_invitation(&invalid_role).is_err());
    }
}
