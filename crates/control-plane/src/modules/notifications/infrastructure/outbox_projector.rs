use crate::modules::edge::domain::events::DomainClaimChanged;
use crate::modules::edge::domain::{DomainClaimState, DomainNamePattern};
use crate::modules::identity::domain::events::MembershipChanged;
use crate::modules::identity::domain::repositories::{
    IMembershipRepository, IResourceGrantRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::integration_events::{IIntegrationEventProjector, OutboxMessage};
use crate::modules::notifications::domain::{
    INotificationAlertPolicyRepository, INotificationRepository, Notification,
    NotificationAlertSource, NotificationScope, NotificationSeverity,
};
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, RepositoryError};
use async_trait::async_trait;
use std::sync::Arc;

pub struct OutboxNotificationProjector {
    notifications: Arc<dyn INotificationRepository>,
    memberships: Arc<dyn IMembershipRepository>,
    alert_policies: Option<Arc<dyn INotificationAlertPolicyRepository>>,
    resource_grants: Option<Arc<dyn IResourceGrantRepository>>,
}

impl OutboxNotificationProjector {
    pub fn new(
        notifications: Arc<dyn INotificationRepository>,
        memberships: Arc<dyn IMembershipRepository>,
    ) -> Self {
        Self {
            notifications,
            memberships,
            alert_policies: None,
            resource_grants: None,
        }
    }

    pub fn with_alert_policies(
        mut self,
        alert_policies: Arc<dyn INotificationAlertPolicyRepository>,
        resource_grants: Arc<dyn IResourceGrantRepository>,
    ) -> Self {
        self.alert_policies = Some(alert_policies);
        self.resource_grants = Some(resource_grants);
        self
    }

    async fn notifications_for(
        &self,
        message: &OutboxMessage,
    ) -> Result<Vec<Notification>, RepositoryError> {
        if message.schema_version != 1 {
            return Ok(Vec::new());
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
            "edge.domain-claim.rejected" | "edge.domain-claim.verified" => {
                return self.domain_claim_notifications(message).await;
            }
            _ => return Ok(Vec::new()),
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
            return Ok(Vec::new());
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
        .map(|notification| vec![notification])
        .map_err(RepositoryError::Storage)
    }

    async fn domain_claim_notifications(
        &self,
        message: &OutboxMessage,
    ) -> Result<Vec<Notification>, RepositoryError> {
        let (Some(alert_policies), Some(resource_grants)) =
            (&self.alert_policies, &self.resource_grants)
        else {
            return Ok(Vec::new());
        };
        let payload = decode_domain_claim(message)?;
        let source = NotificationAlertSource::EdgeDomainClaimStatusV1;
        let policies = alert_policies
            .list_active_alert_policies_for_source(
                OrganizationId::from_uuid(message.organization_id),
                source,
                payload.project_id,
                payload.environment_id,
                message.occurred_at,
            )
            .await?;
        let scope = NotificationScope::Environment {
            project_id: payload.project_id,
            environment_id: payload.environment_id,
        };
        let mut notifications = Vec::with_capacity(policies.len());
        for policy in policies {
            let Some(membership) = self
                .memberships
                .find_active_membership_by_principal(
                    policy.organization_id,
                    policy.recipient_principal_id,
                )
                .await?
            else {
                continue;
            };
            let grants = resource_grants
                .list_active_resource_grants_for_membership(policy.organization_id, membership.id)
                .await?;
            let access = ResourceAccessEvaluator::for_membership(
                membership.role,
                grants.into_iter().map(|grant| grant.scope),
            );
            if !scope.is_visible_to(&access) {
                continue;
            }

            let (severity, title, body) = match payload.state {
                DomainClaimState::Rejected => (
                    NotificationSeverity::Warning,
                    "Domain claim rejected".to_owned(),
                    format!(
                        "{} could not be verified. Review its domain ownership challenge.",
                        payload.pattern
                    ),
                ),
                DomainClaimState::Verified => {
                    if !policy.definition.spec().notify_on_recovery {
                        continue;
                    }
                    let latest = self
                        .notifications
                        .latest_alert_source_projection(
                            policy.organization_id,
                            policy.recipient_principal_id,
                            source,
                            message.aggregate_id,
                            policy.created_at,
                            message.aggregate_version,
                        )
                        .await?;
                    if latest
                        .as_ref()
                        .map(|notification| notification.source_event_key.as_str())
                        != Some("edge.domain-claim.rejected")
                    {
                        continue;
                    }
                    (
                        NotificationSeverity::Information,
                        "Domain claim recovered".to_owned(),
                        format!("{} is now verified.", payload.pattern),
                    )
                }
                DomainClaimState::Pending | DomainClaimState::Revoked => {
                    return Err(RepositoryError::Storage(
                        "notification domain claim source state is unsupported".into(),
                    ));
                }
            };
            notifications.push(
                Notification::project(
                    policy.organization_id,
                    policy.recipient_principal_id,
                    message.event_id,
                    message.event_key.clone(),
                    message.schema_version,
                    message.aggregate_id,
                    message.aggregate_version,
                    message.correlation_id,
                    severity,
                    title,
                    body,
                    scope,
                    message.occurred_at,
                    message.occurred_at,
                )
                .map_err(RepositoryError::Storage)?,
            );
        }
        Ok(notifications)
    }
}

#[async_trait]
impl IIntegrationEventProjector for OutboxNotificationProjector {
    async fn project(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        for notification in self.notifications_for(message).await? {
            self.notifications.project(notification).await?;
        }
        Ok(())
    }
}

fn decode_domain_claim(message: &OutboxMessage) -> Result<DomainClaimChanged, RepositoryError> {
    let payload: DomainClaimChanged =
        serde_json::from_value(message.payload.clone()).map_err(|error| {
            RepositoryError::Storage(format!(
                "notification source domain claim payload is invalid: {error}"
            ))
        })?;
    let expected_state = match message.event_key.as_str() {
        "edge.domain-claim.rejected" => DomainClaimState::Rejected,
        "edge.domain-claim.verified" => DomainClaimState::Verified,
        _ => {
            return Err(RepositoryError::Storage(
                "notification domain claim source key is unsupported".into(),
            ))
        }
    };
    let valid_failure = match expected_state {
        DomainClaimState::Rejected => payload.failure.as_deref().is_some_and(|failure| {
            !failure.is_empty()
                && failure.len() <= 4_096
                && failure.trim() == failure
                && !failure.contains(['\0', '\r', '\n'])
        }),
        DomainClaimState::Verified => payload.failure.is_none(),
        DomainClaimState::Pending | DomainClaimState::Revoked => false,
    };
    if payload.organization_id.as_uuid() != message.organization_id
        || payload.domain_claim_id.as_uuid() != message.aggregate_id
        || payload.project_id.as_uuid().is_nil()
        || payload.environment_id.as_uuid().is_nil()
        || payload.state != expected_state
        || message.aggregate_version < 2
        || DomainNamePattern::parse(payload.pattern.clone()).is_err()
        || !valid_failure
    {
        return Err(RepositoryError::Storage(
            "notification source domain claim payload identity is inconsistent".into(),
        ));
    }
    Ok(payload)
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
        IdentityPrincipal, IdentityPrincipalKind, Membership, ResourceGrant,
    };
    use crate::modules::identity::domain::repositories::{
        ChangeMembershipRoleWrite, CreateMembershipWrite, CreateResourceGrantWrite,
        IResourceGrantRepository, MembershipRecord, RevokeMembershipWrite,
        RevokeResourceGrantWrite,
    };
    use crate::modules::identity::domain::value_objects::ResourceGrantScope;
    use crate::modules::integration_events::{
        EventPublishError, IEventPublisher, IOutboxRepository, OutboxRelay, OutboxRelayConfig,
    };
    use crate::modules::notifications::{
        CreateNotificationAlertPolicyWrite, InMemoryNotificationRepository,
        NotificationAlertPolicy, NotificationAlertPolicyDefinition, NotificationAlertPolicyEvent,
        NotificationAlertPolicySpec, RevokeNotificationAlertPolicyWrite,
    };
    use crate::modules::shared_kernel::domain::{
        DomainClaimId, EnvironmentId, IdempotencyRequest, IdempotentWrite, MembershipId,
        NotificationAlertPolicyId, ProjectId, ResourceGrantId, ResourceName,
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
                .find(|grant| {
                    grant.organization_id == organization_id && grant.id == resource_grant_id
                })
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
        let definition =
            NotificationAlertPolicyDefinition::from_spec(NotificationAlertPolicySpec {
                source: NotificationAlertSource::EdgeDomainClaimStatusV1,
                project_id,
                environment_id,
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

    #[allow(clippy::too_many_arguments)]
    fn domain_claim_message(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        claim_id: DomainClaimId,
        event_key: &str,
        state: DomainClaimState,
        failure: Option<&str>,
        aggregate_version: u64,
        occurred_at: DateTime<Utc>,
    ) -> OutboxMessage {
        OutboxMessage {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            organization_id: organization_id.as_uuid(),
            aggregate_id: claim_id.as_uuid(),
            aggregate_version,
            occurred_at,
            correlation_id: Uuid::now_v7(),
            causation_id: None,
            payload: serde_json::to_value(DomainClaimChanged {
                organization_id,
                project_id,
                environment_id,
                domain_claim_id: claim_id,
                pattern: "app.example.com".into(),
                state,
                failure: failure.map(str::to_owned),
            })
            .expect("domain claim payload"),
            delivery_attempts: 1,
        }
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

    #[tokio::test]
    async fn domain_claim_rejection_and_recovery_are_personal_deterministic_projections() {
        let organization_id = OrganizationId::new();
        let recipient = PrincipalId::new();
        let membership_id = MembershipId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let claim_id = DomainClaimId::new();
        let created_at = Utc::now();
        let notifications = Arc::new(InMemoryNotificationRepository::new());
        create_alert_policy(
            notifications.as_ref(),
            organization_id,
            recipient,
            project_id,
            environment_id,
            true,
            created_at,
        )
        .await;
        let projector = OutboxNotificationProjector::new(
            notifications.clone(),
            membership_lookup(organization_id, membership_id, recipient, created_at),
        )
        .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
        let rejected = domain_claim_message(
            organization_id,
            project_id,
            environment_id,
            claim_id,
            "edge.domain-claim.rejected",
            DomainClaimState::Rejected,
            Some("private provider detail must not leak"),
            2,
            created_at + chrono::Duration::seconds(1),
        );

        projector
            .project(&rejected)
            .await
            .expect("project rejection");
        projector
            .project(&rejected)
            .await
            .expect("replay rejection projection");
        let projected = notifications
            .list_page(organization_id, recipient, false, None, 50)
            .await
            .expect("rejection notifications");
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].severity, NotificationSeverity::Warning);
        assert_eq!(projected[0].title, "Domain claim rejected");
        assert!(!projected[0].body.contains("private provider detail"));
        assert_eq!(
            projected[0].scope,
            NotificationScope::Environment {
                project_id,
                environment_id,
            }
        );

        let recovered = domain_claim_message(
            organization_id,
            project_id,
            environment_id,
            claim_id,
            "edge.domain-claim.verified",
            DomainClaimState::Verified,
            None,
            3,
            created_at + chrono::Duration::seconds(2),
        );
        projector
            .project(&recovered)
            .await
            .expect("project recovery");
        projector
            .project(&recovered)
            .await
            .expect("replay recovery projection");
        let projected = notifications
            .list_page(organization_id, recipient, false, None, 50)
            .await
            .expect("recovery notifications");
        assert_eq!(projected.len(), 2);
        assert_eq!(projected[0].severity, NotificationSeverity::Information);
        assert_eq!(projected[0].title, "Domain claim recovered");
        assert_eq!(projected[0].source_aggregate_version, 3);
        assert_eq!(projected[1].source_aggregate_version, 2);
    }

    #[tokio::test]
    async fn domain_claim_recovery_requires_a_post_policy_rejection_and_opt_in() {
        let organization_id = OrganizationId::new();
        let recipient = PrincipalId::new();
        let membership_id = MembershipId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let created_at = Utc::now();
        let notifications = Arc::new(InMemoryNotificationRepository::new());
        create_alert_policy(
            notifications.as_ref(),
            organization_id,
            recipient,
            project_id,
            environment_id,
            true,
            created_at,
        )
        .await;
        let projector = OutboxNotificationProjector::new(
            notifications.clone(),
            membership_lookup(organization_id, membership_id, recipient, created_at),
        )
        .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));

        let historical_claim = DomainClaimId::new();
        projector
            .project(&domain_claim_message(
                organization_id,
                project_id,
                environment_id,
                historical_claim,
                "edge.domain-claim.rejected",
                DomainClaimState::Rejected,
                Some("historical rejection"),
                2,
                created_at - chrono::Duration::seconds(1),
            ))
            .await
            .expect("historical rejection is ignored");
        projector
            .project(&domain_claim_message(
                organization_id,
                project_id,
                environment_id,
                historical_claim,
                "edge.domain-claim.verified",
                DomainClaimState::Verified,
                None,
                3,
                created_at + chrono::Duration::seconds(1),
            ))
            .await
            .expect("recovery without projected rejection is ignored");
        projector
            .project(&domain_claim_message(
                organization_id,
                project_id,
                environment_id,
                DomainClaimId::new(),
                "edge.domain-claim.verified",
                DomainClaimState::Verified,
                None,
                2,
                created_at + chrono::Duration::seconds(1),
            ))
            .await
            .expect("initial verification is ignored");
        assert!(notifications
            .list_page(organization_id, recipient, false, None, 50)
            .await
            .expect("notifications")
            .is_empty());

        let no_recovery_recipient = PrincipalId::new();
        let no_recovery_membership_id = MembershipId::new();
        let no_recovery_environment_id = EnvironmentId::new();
        create_alert_policy(
            notifications.as_ref(),
            organization_id,
            no_recovery_recipient,
            project_id,
            no_recovery_environment_id,
            false,
            created_at,
        )
        .await;
        let no_recovery_projector = OutboxNotificationProjector::new(
            notifications.clone(),
            membership_lookup(
                organization_id,
                no_recovery_membership_id,
                no_recovery_recipient,
                created_at,
            ),
        )
        .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
        let claim_id = DomainClaimId::new();
        no_recovery_projector
            .project(&domain_claim_message(
                organization_id,
                project_id,
                no_recovery_environment_id,
                claim_id,
                "edge.domain-claim.rejected",
                DomainClaimState::Rejected,
                Some("rejected"),
                2,
                created_at + chrono::Duration::seconds(1),
            ))
            .await
            .expect("project rejection");
        no_recovery_projector
            .project(&domain_claim_message(
                organization_id,
                project_id,
                no_recovery_environment_id,
                claim_id,
                "edge.domain-claim.verified",
                DomainClaimState::Verified,
                None,
                3,
                created_at + chrono::Duration::seconds(2),
            ))
            .await
            .expect("recovery opt-out is ignored");
        let projected = notifications
            .list_page(organization_id, no_recovery_recipient, false, None, 50)
            .await
            .expect("notifications");
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].source_event_key, "edge.domain-claim.rejected");
    }

    #[tokio::test]
    async fn domain_claim_alerts_recheck_policy_membership_and_resource_grants() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let created_at = Utc::now();
        let notifications = Arc::new(InMemoryNotificationRepository::new());

        let restricted_recipient = PrincipalId::new();
        let restricted_membership_id = MembershipId::new();
        create_alert_policy(
            notifications.as_ref(),
            organization_id,
            restricted_recipient,
            project_id,
            environment_id,
            true,
            created_at,
        )
        .await;
        let restricted_projector = OutboxNotificationProjector::new(
            notifications.clone(),
            membership_lookup_with_role(
                organization_id,
                restricted_membership_id,
                restricted_recipient,
                MembershipRole::Restricted,
                true,
                created_at,
            ),
        )
        .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
        restricted_projector
            .project(&domain_claim_message(
                organization_id,
                project_id,
                environment_id,
                DomainClaimId::new(),
                "edge.domain-claim.rejected",
                DomainClaimState::Rejected,
                Some("rejected"),
                2,
                created_at + chrono::Duration::seconds(1),
            ))
            .await
            .expect("missing grant is ignored");
        assert!(notifications
            .list_page(organization_id, restricted_recipient, false, None, 50,)
            .await
            .expect("notifications")
            .is_empty());

        let granted_recipient = PrincipalId::new();
        let granted_membership_id = MembershipId::new();
        create_alert_policy(
            notifications.as_ref(),
            organization_id,
            granted_recipient,
            project_id,
            environment_id,
            true,
            created_at,
        )
        .await;
        let grant = ResourceGrant::create(
            ResourceGrantId::new(),
            organization_id,
            granted_membership_id,
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            },
            created_at,
        );
        let granted_projector = OutboxNotificationProjector::new(
            notifications.clone(),
            membership_lookup_with_role(
                organization_id,
                granted_membership_id,
                granted_recipient,
                MembershipRole::Restricted,
                true,
                created_at,
            ),
        )
        .with_alert_policies(notifications.clone(), resource_grants(vec![grant]));
        granted_projector
            .project(&domain_claim_message(
                organization_id,
                project_id,
                environment_id,
                DomainClaimId::new(),
                "edge.domain-claim.rejected",
                DomainClaimState::Rejected,
                Some("rejected"),
                2,
                created_at + chrono::Duration::seconds(1),
            ))
            .await
            .expect("matching grant projects alert");
        assert_eq!(
            notifications
                .list_page(organization_id, granted_recipient, false, None, 50)
                .await
                .expect("notifications")
                .len(),
            1
        );

        let revoked_recipient = PrincipalId::new();
        let revoked_membership_id = MembershipId::new();
        create_alert_policy(
            notifications.as_ref(),
            organization_id,
            revoked_recipient,
            project_id,
            environment_id,
            true,
            created_at,
        )
        .await;
        let revoked_member_projector = OutboxNotificationProjector::new(
            notifications.clone(),
            membership_lookup_with_role(
                organization_id,
                revoked_membership_id,
                revoked_recipient,
                MembershipRole::Member,
                false,
                created_at,
            ),
        )
        .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
        revoked_member_projector
            .project(&domain_claim_message(
                organization_id,
                project_id,
                environment_id,
                DomainClaimId::new(),
                "edge.domain-claim.rejected",
                DomainClaimState::Rejected,
                Some("rejected"),
                2,
                created_at + chrono::Duration::seconds(1),
            ))
            .await
            .expect("revoked membership is ignored");
        assert!(notifications
            .list_page(organization_id, revoked_recipient, false, None, 50)
            .await
            .expect("notifications")
            .is_empty());

        let revoked_policy_recipient = PrincipalId::new();
        let policy = create_alert_policy(
            notifications.as_ref(),
            organization_id,
            revoked_policy_recipient,
            project_id,
            environment_id,
            true,
            created_at,
        )
        .await;
        revoke_alert_policy(
            notifications.as_ref(),
            &policy,
            created_at + chrono::Duration::seconds(1),
        )
        .await;
        let revoked_policy_projector = OutboxNotificationProjector::new(
            notifications.clone(),
            membership_lookup(
                organization_id,
                MembershipId::new(),
                revoked_policy_recipient,
                created_at,
            ),
        )
        .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
        revoked_policy_projector
            .project(&domain_claim_message(
                organization_id,
                project_id,
                environment_id,
                DomainClaimId::new(),
                "edge.domain-claim.rejected",
                DomainClaimState::Rejected,
                Some("rejected"),
                2,
                created_at + chrono::Duration::seconds(2),
            ))
            .await
            .expect("revoked policy is ignored");
        assert!(notifications
            .list_page(organization_id, revoked_policy_recipient, false, None, 50,)
            .await
            .expect("notifications")
            .is_empty());
    }

    #[test]
    fn malformed_domain_claim_payloads_fail_closed() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let claim_id = DomainClaimId::new();
        let mut message = domain_claim_message(
            organization_id,
            project_id,
            environment_id,
            claim_id,
            "edge.domain-claim.rejected",
            DomainClaimState::Rejected,
            Some("rejected"),
            2,
            Utc::now(),
        );
        message.payload["unexpected"] = serde_json::json!(true);
        assert!(decode_domain_claim(&message).is_err());

        let mut inconsistent = domain_claim_message(
            organization_id,
            project_id,
            environment_id,
            claim_id,
            "edge.domain-claim.verified",
            DomainClaimState::Rejected,
            Some("rejected"),
            3,
            Utc::now(),
        );
        assert!(decode_domain_claim(&inconsistent).is_err());
        inconsistent.payload["state"] = serde_json::json!("verified");
        assert!(decode_domain_claim(&inconsistent).is_err());
    }
}
