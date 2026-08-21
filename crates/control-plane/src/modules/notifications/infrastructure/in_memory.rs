use crate::modules::notifications::domain::{
    CreateNotificationAlertPolicyWrite, CreateOutboundNotificationSubscriptionWrite,
    INotificationAlertPolicyRepository, INotificationRepository,
    IOutboundNotificationDeliveryRepository, IOutboundNotificationRepository,
    MarkNotificationReadWrite, Notification, NotificationAlertPolicy,
    NotificationAlertPolicyCursor, NotificationAlertSource, NotificationCursor,
    OutboundNotificationDelivery, OutboundNotificationDeliveryAdmission,
    OutboundNotificationSubscription, OutboundNotificationSubscriptionCursor,
    OutboundNotificationTerminalReceipt, RevokeNotificationAlertPolicyWrite,
    RevokeOutboundNotificationSubscriptionWrite,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, NotificationAlertPolicyId, NotificationId,
    NotificationSubscriptionId, OrganizationId, PrincipalId, ProjectId, RepositoryError,
    Sha256Digest,
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
    subscription_idempotency:
        BTreeMap<(String, String), (String, OutboundNotificationSubscription)>,
    subscriptions:
        BTreeMap<(OrganizationId, NotificationSubscriptionId), OutboundNotificationSubscription>,
    alert_policy_idempotency: BTreeMap<(String, String), (String, NotificationAlertPolicy)>,
    alert_policies: BTreeMap<(OrganizationId, NotificationAlertPolicyId), NotificationAlertPolicy>,
    outbound_deliveries: BTreeMap<(OrganizationId, uuid::Uuid), StoredOutboundDelivery>,
    outbox: Vec<a3s_cloud_contracts::DomainEventEnvelope>,
}

#[derive(Clone)]
struct StoredOutboundDelivery {
    delivery: OutboundNotificationDelivery,
    subscription_id: NotificationSubscriptionId,
    payload_digest: Sha256Digest,
    receipt: Option<OutboundNotificationTerminalReceipt>,
}

impl InMemoryNotificationRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<a3s_cloud_contracts::DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }

    pub async fn outbound_deliveries(&self) -> Vec<OutboundNotificationDelivery> {
        self.state
            .read()
            .await
            .outbound_deliveries
            .values()
            .map(|stored| stored.delivery.clone())
            .collect()
    }

    pub async fn outbound_receipts(&self) -> Vec<OutboundNotificationTerminalReceipt> {
        self.state
            .read()
            .await
            .outbound_deliveries
            .values()
            .filter_map(|stored| stored.receipt.clone())
            .collect()
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
        let mut outbound = Vec::new();
        for subscription in state
            .subscriptions
            .values()
            .filter(|subscription| subscription.matches(&notification))
        {
            let delivery = subscription
                .definition
                .delivery_for(&notification)
                .map_err(RepositoryError::Storage)?;
            if state
                .outbound_deliveries
                .contains_key(&(delivery.organization_id(), delivery.id()))
                || outbound.iter().any(
                    |(candidate, _, _, _): &(
                        OutboundNotificationDelivery,
                        NotificationSubscriptionId,
                        Sha256Digest,
                        a3s_cloud_contracts::DomainEventEnvelope,
                    )| candidate.id() == delivery.id(),
                )
            {
                return Err(RepositoryError::Conflict(
                    "outbound notification delivery identity is already in use".into(),
                ));
            }
            let payload_digest = Sha256Digest::from_bytes(
                &delivery
                    .canonical_payload()
                    .map_err(RepositoryError::Storage)?,
            );
            let event = delivery
                .requested_event()
                .map_err(RepositoryError::Storage)?;
            outbound.push((delivery, subscription.id, payload_digest, event));
        }
        state.source_events.insert(source_key, notification.id);
        state.notifications.insert(key, notification);
        for (delivery, subscription_id, payload_digest, event) in outbound {
            state.outbound_deliveries.insert(
                (delivery.organization_id(), delivery.id()),
                StoredOutboundDelivery {
                    delivery,
                    subscription_id,
                    payload_digest,
                    receipt: None,
                },
            );
            state.outbox.push(event);
        }
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

    async fn latest_alert_source_projection(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        source: NotificationAlertSource,
        source_aggregate_id: uuid::Uuid,
        not_before: chrono::DateTime<chrono::Utc>,
        before_aggregate_version: u64,
    ) -> Result<Option<Notification>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .notifications
            .values()
            .filter(|notification| {
                notification.organization_id == organization_id
                    && notification.recipient_principal_id == recipient_principal_id
                    && notification.source_aggregate_id == source_aggregate_id
                    && notification.occurred_at >= not_before
                    && notification.source_aggregate_version < before_aggregate_version
                    && source
                        .event_keys()
                        .contains(&notification.source_event_key.as_str())
            })
            .max_by(|left, right| {
                left.source_aggregate_version
                    .cmp(&right.source_aggregate_version)
                    .then_with(|| left.occurred_at.cmp(&right.occurred_at))
                    .then_with(|| left.id.cmp(&right.id))
            })
            .cloned())
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

#[async_trait]
impl INotificationAlertPolicyRepository for InMemoryNotificationRepository {
    async fn replay_alert_policy_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<NotificationAlertPolicy>>, RepositoryError> {
        idempotency.validate().map_err(RepositoryError::Storage)?;
        let state = self.state.read().await;
        let key = (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        );
        match state.alert_policy_idempotency.get(&key) {
            Some((digest, _)) if digest != &idempotency.request_digest => {
                Err(RepositoryError::IdempotencyConflict)
            }
            Some((_, policy)) => Ok(Some(IdempotentWrite {
                value: policy.clone(),
                replayed: true,
            })),
            None => Ok(None),
        }
    }

    async fn create_alert_policy(
        &self,
        write: CreateNotificationAlertPolicyWrite,
    ) -> Result<IdempotentWrite<NotificationAlertPolicy>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let idempotency_key = (
            write.idempotency.storage_key().0.to_owned(),
            write.idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, policy)) = state.alert_policy_idempotency.get(&idempotency_key) {
            if digest != &write.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: policy.clone(),
                replayed: true,
            });
        }
        let policy = write.policy;
        let key = (policy.organization_id, policy.id);
        if state.alert_policies.contains_key(&key) {
            return Err(RepositoryError::Conflict(
                "notification alert policy ID is already in use".into(),
            ));
        }
        let spec = policy.definition.spec();
        if state.alert_policies.values().any(|existing| {
            let existing_spec = existing.definition.spec();
            existing.is_active()
                && existing.organization_id == policy.organization_id
                && existing.recipient_principal_id == policy.recipient_principal_id
                && existing_spec.source == spec.source
                && existing_spec.project_id == spec.project_id
                && existing_spec.environment_id == spec.environment_id
        }) {
            return Err(RepositoryError::Conflict(
                "an active notification alert policy already owns this exact source scope".into(),
            ));
        }
        state.alert_policies.insert(key, policy.clone());
        state.alert_policy_idempotency.insert(
            idempotency_key,
            (write.idempotency.request_digest, policy.clone()),
        );
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: policy,
            replayed: false,
        })
    }

    async fn revoke_alert_policy(
        &self,
        write: RevokeNotificationAlertPolicyWrite,
    ) -> Result<IdempotentWrite<NotificationAlertPolicy>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let idempotency_key = (
            write.idempotency.storage_key().0.to_owned(),
            write.idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, policy)) = state.alert_policy_idempotency.get(&idempotency_key) {
            if digest != &write.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: policy.clone(),
                replayed: true,
            });
        }
        let key = (write.policy.organization_id, write.policy.id);
        let existing = state
            .alert_policies
            .get(&key)
            .ok_or(RepositoryError::NotFound)?;
        write.validate_against(existing).map_err(|_| {
            RepositoryError::Conflict("notification alert policy changed while revoking".into())
        })?;
        let policy = write.policy;
        state.alert_policies.insert(key, policy.clone());
        state.alert_policy_idempotency.insert(
            idempotency_key,
            (write.idempotency.request_digest, policy.clone()),
        );
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: policy,
            replayed: false,
        })
    }

    async fn find_alert_policy(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        policy_id: NotificationAlertPolicyId,
    ) -> Result<Option<NotificationAlertPolicy>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .alert_policies
            .get(&(organization_id, policy_id))
            .filter(|policy| policy.recipient_principal_id == recipient_principal_id)
            .cloned())
    }

    async fn list_alert_policy_page(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        after: Option<NotificationAlertPolicyCursor>,
        limit: usize,
    ) -> Result<Vec<NotificationAlertPolicy>, RepositoryError> {
        let mut policies = self
            .state
            .read()
            .await
            .alert_policies
            .values()
            .filter(|policy| {
                policy.organization_id == organization_id
                    && policy.recipient_principal_id == recipient_principal_id
                    && after.is_none_or(|cursor| {
                        policy.created_at < cursor.created_at
                            || (policy.created_at == cursor.created_at
                                && policy.id < cursor.policy_id)
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        policies.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        policies.truncate(limit);
        Ok(policies)
    }

    async fn list_active_alert_policies_for_source(
        &self,
        organization_id: OrganizationId,
        source: NotificationAlertSource,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        occurred_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<NotificationAlertPolicy>, RepositoryError> {
        let mut policies = self
            .state
            .read()
            .await
            .alert_policies
            .values()
            .filter(|policy| {
                policy.organization_id == organization_id
                    && policy.matches(source, project_id, environment_id, occurred_at)
            })
            .cloned()
            .collect::<Vec<_>>();
        policies.sort_by_key(|policy| (policy.created_at, policy.id));
        Ok(policies)
    }
}

#[async_trait]
impl IOutboundNotificationRepository for InMemoryNotificationRepository {
    async fn replay_subscription_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<OutboundNotificationSubscription>>, RepositoryError> {
        idempotency.validate().map_err(RepositoryError::Storage)?;
        let state = self.state.read().await;
        let key = (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        );
        match state.subscription_idempotency.get(&key) {
            Some((digest, _)) if digest != &idempotency.request_digest => {
                Err(RepositoryError::IdempotencyConflict)
            }
            Some((_, subscription)) => Ok(Some(IdempotentWrite {
                value: subscription.clone(),
                replayed: true,
            })),
            None => Ok(None),
        }
    }

    async fn create_subscription(
        &self,
        write: CreateOutboundNotificationSubscriptionWrite,
    ) -> Result<IdempotentWrite<OutboundNotificationSubscription>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let idempotency_key = (
            write.idempotency.storage_key().0.to_owned(),
            write.idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, subscription)) = state.subscription_idempotency.get(&idempotency_key) {
            if digest != &write.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: subscription.clone(),
                replayed: true,
            });
        }
        let subscription = write.subscription;
        let key = (subscription.organization_id, subscription.id);
        if state.subscriptions.contains_key(&key) {
            return Err(RepositoryError::Conflict(
                "outbound notification subscription ID is already in use".into(),
            ));
        }
        let spec = subscription.definition.spec();
        if state.subscriptions.values().any(|existing| {
            let existing_spec = existing.definition.spec();
            existing.is_active()
                && existing.organization_id == subscription.organization_id
                && existing.recipient_principal_id == subscription.recipient_principal_id
                && existing_spec.channel == spec.channel
                && existing_spec.target == spec.target
        }) {
            return Err(RepositoryError::Conflict(
                "an active outbound notification subscription already owns this exact target"
                    .into(),
            ));
        }
        state.subscriptions.insert(key, subscription.clone());
        state.subscription_idempotency.insert(
            idempotency_key,
            (write.idempotency.request_digest, subscription.clone()),
        );
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: subscription,
            replayed: false,
        })
    }

    async fn revoke_subscription(
        &self,
        write: RevokeOutboundNotificationSubscriptionWrite,
    ) -> Result<IdempotentWrite<OutboundNotificationSubscription>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let idempotency_key = (
            write.idempotency.storage_key().0.to_owned(),
            write.idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, subscription)) = state.subscription_idempotency.get(&idempotency_key) {
            if digest != &write.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: subscription.clone(),
                replayed: true,
            });
        }
        let key = (write.subscription.organization_id, write.subscription.id);
        let existing = state
            .subscriptions
            .get(&key)
            .ok_or(RepositoryError::NotFound)?;
        write.validate_against(existing).map_err(|_| {
            RepositoryError::Conflict(
                "outbound notification subscription changed while revoking".into(),
            )
        })?;
        let subscription = write.subscription;
        state.subscriptions.insert(key, subscription.clone());
        state.subscription_idempotency.insert(
            idempotency_key,
            (write.idempotency.request_digest, subscription.clone()),
        );
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: subscription,
            replayed: false,
        })
    }

    async fn find_subscription(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        subscription_id: NotificationSubscriptionId,
    ) -> Result<Option<OutboundNotificationSubscription>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .subscriptions
            .get(&(organization_id, subscription_id))
            .filter(|subscription| subscription.recipient_principal_id == recipient_principal_id)
            .cloned())
    }

    async fn list_subscription_page(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        after: Option<OutboundNotificationSubscriptionCursor>,
        limit: usize,
    ) -> Result<Vec<OutboundNotificationSubscription>, RepositoryError> {
        let mut subscriptions = self
            .state
            .read()
            .await
            .subscriptions
            .values()
            .filter(|subscription| {
                subscription.organization_id == organization_id
                    && subscription.recipient_principal_id == recipient_principal_id
                    && after.is_none_or(|cursor| {
                        subscription.created_at < cursor.created_at
                            || (subscription.created_at == cursor.created_at
                                && subscription.id < cursor.subscription_id)
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        subscriptions.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        subscriptions.truncate(limit);
        Ok(subscriptions)
    }
}

#[async_trait]
impl IOutboundNotificationDeliveryRepository for InMemoryNotificationRepository {
    async fn admit_delivery(
        &self,
        delivery: &OutboundNotificationDelivery,
    ) -> Result<Option<OutboundNotificationDeliveryAdmission>, RepositoryError> {
        delivery.validate().map_err(RepositoryError::Storage)?;
        let payload_digest = Sha256Digest::from_bytes(
            &delivery
                .canonical_payload()
                .map_err(RepositoryError::Storage)?,
        );
        let state = self.state.read().await;
        let Some(stored) = state
            .outbound_deliveries
            .get(&(delivery.organization_id(), delivery.id()))
        else {
            return Ok(None);
        };
        if stored.delivery != *delivery
            || stored.payload_digest != payload_digest
            || !state
                .subscriptions
                .contains_key(&(delivery.organization_id(), stored.subscription_id))
        {
            return Ok(None);
        }
        Ok(Some(match &stored.receipt {
            Some(receipt) => OutboundNotificationDeliveryAdmission::Terminal(receipt.clone()),
            None => OutboundNotificationDeliveryAdmission::Pending,
        }))
    }

    async fn settle_delivery(
        &self,
        delivery: &OutboundNotificationDelivery,
        receipt: OutboundNotificationTerminalReceipt,
    ) -> Result<bool, RepositoryError> {
        receipt
            .validate_against(delivery)
            .map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let stored = state
            .outbound_deliveries
            .get_mut(&(delivery.organization_id(), delivery.id()))
            .ok_or(RepositoryError::NotFound)?;
        if stored.delivery != *delivery {
            return Err(RepositoryError::Conflict(
                "outbound notification delivery fact changed before settlement".into(),
            ));
        }
        match &stored.receipt {
            Some(existing) if existing == &receipt => Ok(false),
            Some(_) => Err(RepositoryError::Conflict(
                "outbound notification delivery already has another terminal receipt".into(),
            )),
            None => {
                stored.receipt = Some(receipt);
                Ok(true)
            }
        }
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
