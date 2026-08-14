use crate::modules::identity::domain::events::MembershipChanged;
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
            _ => return Ok(None),
        };

        // The organization-scoped inbox is reachable only by active members. Invitation and
        // revocation facts therefore remain in their existing lifecycle surfaces instead of
        // creating dead inbox records. A delayed fact is also skipped if access has since ended.
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
                RepositoryError::Storage("notification source membership no longer exists".into())
            })?;
        if membership.membership.principal_id != recipient {
            return Err(RepositoryError::Storage(
                "notification source membership principal is inconsistent".into(),
            ));
        }
        if !membership.membership.is_active() {
            return Ok(None);
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
    use crate::modules::identity::domain::entities::{
        IdentityPrincipal, IdentityPrincipalKind, Membership,
    };
    use crate::modules::identity::domain::repositories::{
        ChangeMembershipRoleWrite, CreateMembershipWrite, MembershipRecord, RevokeMembershipWrite,
    };
    use crate::modules::integration_events::{
        EventPublishError, IEventPublisher, IOutboxRepository, OutboxRelay, OutboxRelayConfig,
    };
    use crate::modules::notifications::InMemoryNotificationRepository;
    use crate::modules::shared_kernel::domain::{IdempotentWrite, MembershipId, ResourceName};
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    struct MembershipLookup {
        record: MembershipRecord,
    }

    #[async_trait]
    impl IMembershipRepository for MembershipLookup {
        async fn create_membership(
            &self,
            _write: CreateMembershipWrite,
        ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError> {
            unreachable!("projection tests only perform membership lookup")
        }

        async fn find_membership(
            &self,
            organization_id: OrganizationId,
            membership_id: MembershipId,
        ) -> Result<Option<MembershipRecord>, RepositoryError> {
            Ok((self.record.membership.organization_id == organization_id
                && self.record.membership.id == membership_id)
                .then(|| self.record.clone()))
        }

        async fn list_memberships(
            &self,
            _organization_id: OrganizationId,
        ) -> Result<Vec<MembershipRecord>, RepositoryError> {
            unreachable!("projection tests only perform membership lookup")
        }

        async fn find_active_membership_by_principal(
            &self,
            _organization_id: OrganizationId,
            _principal_id: PrincipalId,
        ) -> Result<Option<Membership>, RepositoryError> {
            unreachable!("projection tests only perform membership lookup")
        }

        async fn change_membership_role(
            &self,
            _write: ChangeMembershipRoleWrite,
        ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError> {
            unreachable!("projection tests only perform membership lookup")
        }

        async fn revoke_membership(
            &self,
            _write: RevokeMembershipWrite,
        ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError> {
            unreachable!("projection tests only perform membership lookup")
        }
    }

    fn membership_lookup(
        organization_id: OrganizationId,
        membership_id: MembershipId,
        recipient: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Arc<dyn IMembershipRepository> {
        Arc::new(MembershipLookup {
            record: MembershipRecord {
                principal: IdentityPrincipal::create(
                    recipient,
                    IdentityPrincipalKind::Human,
                    ResourceName::parse("Notification recipient").expect("principal name"),
                    created_at,
                ),
                membership: Membership::create(
                    membership_id,
                    organization_id,
                    recipient,
                    MembershipRole::Member,
                    created_at,
                ),
            },
        })
    }

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
            Ok(if state.published {
                Vec::new()
            } else {
                vec![state.message.clone()]
            })
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
        let membership_id = MembershipId::new();
        let occurred_at = Utc::now();
        let message = OutboxMessage {
            event_id: Uuid::now_v7(),
            event_key: "identity.membership.created".into(),
            schema_version: 1,
            organization_id: organization_id.as_uuid(),
            aggregate_id: membership_id.as_uuid(),
            aggregate_version: 1,
            occurred_at,
            correlation_id: Uuid::now_v7(),
            causation_id: None,
            payload: serde_json::json!({
                "membership_id": membership_id,
                "principal_id": recipient,
                "role": "member"
            }),
            delivery_attempts: 1,
        };
        let outbox = Arc::new(RetryingOutboxRepository::new(message));
        let publisher = Arc::new(FailOncePublisher::default());
        let notifications = Arc::new(InMemoryNotificationRepository::new());
        let projector = Arc::new(OutboxNotificationProjector::new(
            notifications.clone(),
            membership_lookup(organization_id, membership_id, recipient, occurred_at),
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
            event_key: "identity.membership.created".into(),
            schema_version: 1,
            organization_id: Uuid::now_v7(),
            aggregate_id,
            aggregate_version: 1,
            occurred_at: Utc::now(),
            correlation_id: Uuid::now_v7(),
            causation_id: None,
            payload: serde_json::json!({
                "membership_id": Uuid::now_v7(),
                "principal_id": Uuid::now_v7(),
                "role": "member"
            }),
            delivery_attempts: 1,
        };
        assert!(decode_membership(&message).is_err());

        let mut invalid_role = message;
        invalid_role.payload["membership_id"] = serde_json::json!(aggregate_id);
        invalid_role.payload["role"] = serde_json::json!("super-admin");
        assert!(decode_membership(&invalid_role).is_err());
    }

    #[tokio::test]
    async fn inaccessible_identity_lifecycle_facts_are_not_projected() {
        let organization_id = OrganizationId::new();
        let recipient = PrincipalId::new();
        let membership_id = MembershipId::new();
        let occurred_at = Utc::now();
        let notifications = Arc::new(InMemoryNotificationRepository::new());
        let projector = OutboxNotificationProjector::new(
            notifications.clone(),
            membership_lookup(organization_id, membership_id, recipient, occurred_at),
        );

        for event_key in [
            "identity.membership-invitation.created",
            "identity.membership-invitation.revoked",
            "identity.membership.revoked",
        ] {
            projector
                .project(&OutboxMessage {
                    event_id: Uuid::now_v7(),
                    event_key: event_key.into(),
                    schema_version: 1,
                    organization_id: organization_id.as_uuid(),
                    aggregate_id: membership_id.as_uuid(),
                    aggregate_version: 1,
                    occurred_at,
                    correlation_id: Uuid::now_v7(),
                    causation_id: None,
                    payload: serde_json::json!({}),
                    delivery_attempts: 1,
                })
                .await
                .expect("unsupported lifecycle fact is a successful no-op");
        }

        assert!(notifications
            .list_page(organization_id, recipient, false, None, 50)
            .await
            .expect("notifications")
            .is_empty());
    }
}
