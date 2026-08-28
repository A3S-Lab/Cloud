use super::*;
use crate::modules::identity::domain::entities::{
    IdentityPrincipal, IdentityPrincipalKind, Membership, ResourceGrant,
};
use crate::modules::identity::domain::repositories::{
    ChangeMembershipRoleWrite, CreateMembershipWrite, CreateResourceGrantWrite,
    IResourceGrantRepository, MembershipRecord, RevokeMembershipWrite, RevokeResourceGrantWrite,
};
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::integration_events::{
    EventPublishError, IEventPublisher, IOutboxRepository, OutboxRelay, OutboxRelayConfig,
};
use crate::modules::notifications::{
    CreateNotificationAlertPolicyWrite, InMemoryNotificationRepository, NotificationAlertPolicy,
    NotificationAlertPolicyDefinition, NotificationAlertPolicyEvent, NotificationAlertPolicySpec,
    RevokeNotificationAlertPolicyWrite,
};
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayCertificateId, IdempotencyRequest, IdempotentWrite,
    MembershipId, NodeId, NotificationAlertPolicyId, ProjectId, ResourceGrantId, ResourceName,
    RouteId, WorkloadId,
};
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
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Option<Membership>, RepositoryError> {
        Ok((self.record.membership.organization_id == organization_id
            && self.record.membership.principal_id == principal_id
            && self.record.membership.is_active())
        .then(|| self.record.membership.clone()))
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
    membership_lookup_with_role(
        organization_id,
        membership_id,
        recipient,
        MembershipRole::Member,
        true,
        created_at,
    )
}

fn membership_lookup_with_role(
    organization_id: OrganizationId,
    membership_id: MembershipId,
    recipient: PrincipalId,
    role: MembershipRole,
    active: bool,
    created_at: DateTime<Utc>,
) -> Arc<dyn IMembershipRepository> {
    let mut membership =
        Membership::create(membership_id, organization_id, recipient, role, created_at);
    if !active {
        assert!(membership.revoke(created_at));
    }
    Arc::new(MembershipLookup {
        record: MembershipRecord {
            principal: IdentityPrincipal::create(
                recipient,
                IdentityPrincipalKind::Human,
                ResourceName::parse("Notification recipient").expect("principal name"),
                created_at,
            ),
            membership,
        },
    })
}

#[derive(Default)]
struct ResourceGrantLookup {
    grants: Vec<ResourceGrant>,
}

#[async_trait]
impl IResourceGrantRepository for ResourceGrantLookup {
    async fn create_resource_grant(
        &self,
        _write: CreateResourceGrantWrite,
    ) -> Result<IdempotentWrite<ResourceGrant>, RepositoryError> {
        unreachable!("projection tests only perform Resource Grant lookup")
    }

    async fn find_resource_grant(
        &self,
        organization_id: OrganizationId,
        resource_grant_id: ResourceGrantId,
    ) -> Result<Option<ResourceGrant>, RepositoryError> {
        Ok(self
            .grants
            .iter()
            .find(|grant| grant.organization_id == organization_id && grant.id == resource_grant_id)
            .cloned())
    }

    async fn list_resource_grants(
        &self,
        organization_id: OrganizationId,
        membership_id: Option<MembershipId>,
    ) -> Result<Vec<ResourceGrant>, RepositoryError> {
        Ok(self
            .grants
            .iter()
            .filter(|grant| {
                grant.organization_id == organization_id
                    && membership_id.is_none_or(|id| grant.membership_id == id)
            })
            .cloned()
            .collect())
    }

    async fn list_active_resource_grants_for_membership(
        &self,
        organization_id: OrganizationId,
        membership_id: MembershipId,
    ) -> Result<Vec<ResourceGrant>, RepositoryError> {
        Ok(self
            .list_resource_grants(organization_id, Some(membership_id))
            .await?
            .into_iter()
            .filter(ResourceGrant::is_active)
            .collect())
    }

    async fn revoke_resource_grant(
        &self,
        _write: RevokeResourceGrantWrite,
    ) -> Result<IdempotentWrite<ResourceGrant>, RepositoryError> {
        unreachable!("projection tests only perform Resource Grant lookup")
    }
}

fn resource_grants(grants: Vec<ResourceGrant>) -> Arc<dyn IResourceGrantRepository> {
    Arc::new(ResourceGrantLookup { grants })
}

async fn create_alert_policy(
    notifications: &InMemoryNotificationRepository,
    organization_id: OrganizationId,
    recipient: PrincipalId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    notify_on_recovery: bool,
    created_at: DateTime<Utc>,
) -> NotificationAlertPolicy {
    create_alert_policy_for_source(
        notifications,
        organization_id,
        recipient,
        NotificationAlertSource::EdgeDomainClaimStatusV1,
        project_id,
        environment_id,
        notify_on_recovery,
        created_at,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn create_alert_policy_for_source(
    notifications: &InMemoryNotificationRepository,
    organization_id: OrganizationId,
    recipient: PrincipalId,
    source: NotificationAlertSource,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    notify_on_recovery: bool,
    created_at: DateTime<Utc>,
) -> NotificationAlertPolicy {
    let definition = NotificationAlertPolicyDefinition::from_spec(NotificationAlertPolicySpec {
        source,
        target: NotificationAlertPolicyTarget::Environment {
            project_id,
            environment_id,
        },
        notify_on_recovery,
    })
    .expect("alert policy definition");
    let policy = NotificationAlertPolicy::create(
        organization_id,
        NotificationAlertPolicyId::new(),
        recipient,
        definition,
        recipient,
        created_at,
    )
    .expect("alert policy");
    let request_id = Uuid::now_v7();
    notifications
        .create_alert_policy(CreateNotificationAlertPolicyWrite {
            event: NotificationAlertPolicyEvent::envelope(
                "notification.alert-policy.created",
                &policy,
                request_id,
            )
            .expect("alert policy event"),
            policy,
            actor_principal_id: recipient,
            request_id,
            idempotency: IdempotencyRequest::new(
                "notification-alert-policy-create",
                request_id.to_string(),
                b"canonical alert policy create",
            )
            .expect("alert policy idempotency"),
        })
        .await
        .expect("store alert policy")
        .value
}

async fn revoke_alert_policy(
    notifications: &InMemoryNotificationRepository,
    policy: &NotificationAlertPolicy,
    revoked_at: DateTime<Utc>,
) -> NotificationAlertPolicy {
    let revoked = policy
        .revoke(1, policy.recipient_principal_id, revoked_at)
        .expect("revoke alert policy");
    let request_id = Uuid::now_v7();
    notifications
        .revoke_alert_policy(RevokeNotificationAlertPolicyWrite {
            event: NotificationAlertPolicyEvent::envelope(
                "notification.alert-policy.revoked",
                &revoked,
                request_id,
            )
            .expect("alert policy revoke event"),
            policy: revoked,
            expected_version: 1,
            actor_principal_id: policy.recipient_principal_id,
            request_id,
            idempotency: IdempotencyRequest::new(
                "notification-alert-policy-revoke",
                request_id.to_string(),
                b"canonical alert policy revoke",
            )
            .expect("alert policy revoke idempotency"),
        })
        .await
        .expect("store alert policy revoke")
        .value
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
        scope: crate::modules::shared_kernel::domain::ScopeContext::organization(
            crate::modules::shared_kernel::domain::InstallationId::new(),
            crate::modules::shared_kernel::domain::OrganizationId::from_uuid(
                organization_id.as_uuid(),
            ),
        )
        .expect("scope"),
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
        scope: crate::modules::shared_kernel::domain::ScopeContext::organization(
            crate::modules::shared_kernel::domain::InstallationId::new(),
            crate::modules::shared_kernel::domain::OrganizationId::from_uuid(Uuid::now_v7()),
        )
        .expect("scope"),
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
                scope: crate::modules::shared_kernel::domain::ScopeContext::organization(
                    crate::modules::shared_kernel::domain::InstallationId::new(),
                    crate::modules::shared_kernel::domain::OrganizationId::from_uuid(
                        organization_id.as_uuid(),
                    ),
                )
                .expect("scope"),
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

#[path = "outbox_projector_domain_claim_tests.rs"]
mod domain_claim;

#[path = "outbox_projector_gateway_certificate_tests.rs"]
mod gateway_certificate;

#[path = "outbox_projector_gateway_certificate_expiry_tests.rs"]
mod gateway_certificate_expiry;

#[path = "outbox_projector_workload_tests.rs"]
mod workload;

#[path = "outbox_projector_node_availability_tests.rs"]
mod node_availability;
