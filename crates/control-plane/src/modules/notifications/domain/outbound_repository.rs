use super::{
    OutboundNotificationDelivery, OutboundNotificationSubscription,
    OutboundNotificationSubscriptionCursor, OutboundNotificationTerminalReceipt,
};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, NotificationSubscriptionId, OrganizationId, PrincipalId,
    RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundNotificationDeliveryAdmission {
    Pending,
    Terminal(OutboundNotificationTerminalReceipt),
}

#[derive(Debug, Clone)]
pub struct CreateOutboundNotificationSubscriptionWrite {
    pub subscription: OutboundNotificationSubscription,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl CreateOutboundNotificationSubscriptionWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.subscription.validate()?;
        self.idempotency.validate()?;
        validate_subscription_event(
            &self.subscription,
            &self.event,
            self.actor_principal_id,
            self.request_id,
            "notification.outbound-subscription.created",
        )?;
        if self.subscription.aggregate_version != 1 || !self.subscription.is_active() {
            return Err("outbound notification subscription create write is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct RevokeOutboundNotificationSubscriptionWrite {
    pub subscription: OutboundNotificationSubscription,
    pub expected_version: u64,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl RevokeOutboundNotificationSubscriptionWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.subscription.validate()?;
        self.idempotency.validate()?;
        validate_subscription_event(
            &self.subscription,
            &self.event,
            self.actor_principal_id,
            self.request_id,
            "notification.outbound-subscription.revoked",
        )?;
        if self.expected_version != 1
            || self.subscription.aggregate_version != self.expected_version + 1
            || self.subscription.is_active()
        {
            return Err("outbound notification subscription revoke write is invalid".into());
        }
        Ok(())
    }

    pub fn validate_against(
        &self,
        existing: &OutboundNotificationSubscription,
    ) -> Result<(), String> {
        self.validate()?;
        let expected = existing.revoke(
            self.expected_version,
            self.actor_principal_id,
            self.subscription
                .revoked_at
                .expect("validated subscription revoke time"),
        )?;
        if expected != self.subscription {
            return Err("outbound notification subscription revoke transition changed".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutboundNotificationSubscriptionEvent {
    pub subscription_id: NotificationSubscriptionId,
    pub recipient_principal_id: PrincipalId,
    pub definition_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition_schema: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_provider_attempts: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppress_before: Option<chrono::DateTime<chrono::Utc>>,
    pub channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector_project_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector_environment_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector_profile_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector_revision_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient_contact_id: Option<Uuid>,
    pub state: String,
}

impl OutboundNotificationSubscriptionEvent {
    pub fn envelope(
        event_key: &str,
        subscription: &OutboundNotificationSubscription,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        subscription.validate()?;
        let spec = subscription.definition.spec();
        let connector_target = spec.target.connector();
        let schema_version = subscription.definition.schema_version();
        let versioned_budget = (schema_version >= 2).then(|| {
            (
                subscription.definition.definition_schema().to_owned(),
                subscription.definition.maximum_provider_attempts(),
            )
        });
        let occurred_at = subscription.revoked_at.unwrap_or(subscription.created_at);
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: subscription.organization_id.as_uuid(),
            },
            aggregate_id: subscription.id.as_uuid(),
            aggregate_version: subscription.aggregate_version,
            occurred_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                subscription_id: subscription.id,
                recipient_principal_id: subscription.recipient_principal_id,
                definition_digest: subscription.definition.digest().to_string(),
                definition_schema: versioned_budget.as_ref().map(|(schema, _)| schema.clone()),
                maximum_provider_attempts: versioned_budget.map(|(_, budget)| budget),
                suppress_before: subscription.definition.suppress_before(),
                channel: spec.channel.as_str().into(),
                connector_project_id: connector_target.map(|target| target.project_id.as_uuid()),
                connector_environment_id: connector_target
                    .map(|target| target.environment_id.as_uuid()),
                connector_profile_id: connector_target.map(|target| target.profile_id.as_uuid()),
                connector_revision_id: connector_target.map(|target| target.revision_id.as_uuid()),
                recipient_contact_id: spec
                    .target
                    .recipient_contact_id()
                    .map(|contact_id| contact_id.as_uuid()),
                state: if subscription.is_active() {
                    "active".into()
                } else {
                    "revoked".into()
                },
            })
            .map_err(|error| error.to_string())?,
        })
    }
}

#[async_trait]
pub trait IOutboundNotificationDeliveryRepository: Send + Sync {
    /// Admits only an exact delivery fact previously committed by Notifications.
    async fn admit_delivery(
        &self,
        delivery: &OutboundNotificationDelivery,
    ) -> Result<Option<OutboundNotificationDeliveryAdmission>, RepositoryError>;

    /// Persists one monotonic logical terminal receipt. Exact retries are no-ops.
    async fn settle_delivery(
        &self,
        delivery: &OutboundNotificationDelivery,
        receipt: OutboundNotificationTerminalReceipt,
    ) -> Result<bool, RepositoryError>;
}

#[async_trait]
pub trait IOutboundNotificationRepository: IOutboundNotificationDeliveryRepository {
    async fn replay_subscription_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<OutboundNotificationSubscription>>, RepositoryError>;

    async fn create_subscription(
        &self,
        write: CreateOutboundNotificationSubscriptionWrite,
    ) -> Result<IdempotentWrite<OutboundNotificationSubscription>, RepositoryError>;

    async fn revoke_subscription(
        &self,
        write: RevokeOutboundNotificationSubscriptionWrite,
    ) -> Result<IdempotentWrite<OutboundNotificationSubscription>, RepositoryError>;

    async fn find_subscription(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        subscription_id: NotificationSubscriptionId,
    ) -> Result<Option<OutboundNotificationSubscription>, RepositoryError>;

    /// Returns one raw recipient page ordered by creation time and subscription ID descending.
    async fn list_subscription_page(
        &self,
        organization_id: OrganizationId,
        recipient_principal_id: PrincipalId,
        after: Option<OutboundNotificationSubscriptionCursor>,
        limit: usize,
    ) -> Result<Vec<OutboundNotificationSubscription>, RepositoryError>;
}

fn validate_subscription_event(
    subscription: &OutboundNotificationSubscription,
    event: &DomainEventEnvelope,
    actor_principal_id: PrincipalId,
    request_id: Uuid,
    event_key: &str,
) -> Result<(), String> {
    if actor_principal_id != subscription.recipient_principal_id
        || actor_principal_id.as_uuid().is_nil()
        || request_id.is_nil()
        || event.event_key != event_key
        || event.schema_version != subscription.definition.schema_version()
        || event.organization_id() != Some(subscription.organization_id.as_uuid())
        || event.aggregate_id != subscription.id.as_uuid()
        || event.aggregate_version != subscription.aggregate_version
        || event.occurred_at != subscription.revoked_at.unwrap_or(subscription.created_at)
        || event.correlation_id != request_id
        || event.causation_id.is_some()
    {
        return Err("outbound notification subscription event identity is inconsistent".into());
    }
    let payload: OutboundNotificationSubscriptionEvent =
        serde_json::from_value(event.payload.clone()).map_err(|error| {
            format!("outbound notification subscription event is invalid: {error}")
        })?;
    let spec = subscription.definition.spec();
    let connector_target = spec.target.connector();
    let versioned_budget = (subscription.definition.schema_version() >= 2).then(|| {
        (
            subscription.definition.definition_schema(),
            subscription.definition.maximum_provider_attempts(),
        )
    });
    if payload.subscription_id != subscription.id
        || payload.recipient_principal_id != subscription.recipient_principal_id
        || payload.definition_digest != subscription.definition.digest().as_str()
        || payload.definition_schema.as_deref() != versioned_budget.map(|(schema, _)| schema)
        || payload.maximum_provider_attempts
            != versioned_budget.map(|(_, maximum_provider_attempts)| maximum_provider_attempts)
        || payload.suppress_before != subscription.definition.suppress_before()
        || payload.channel != spec.channel.as_str()
        || payload.connector_project_id
            != connector_target.map(|target| target.project_id.as_uuid())
        || payload.connector_environment_id
            != connector_target.map(|target| target.environment_id.as_uuid())
        || payload.connector_profile_id
            != connector_target.map(|target| target.profile_id.as_uuid())
        || payload.connector_revision_id
            != connector_target.map(|target| target.revision_id.as_uuid())
        || payload.recipient_contact_id
            != spec
                .target
                .recipient_contact_id()
                .map(|contact_id| contact_id.as_uuid())
        || payload.state
            != if subscription.is_active() {
                "active"
            } else {
                "revoked"
            }
    {
        return Err("outbound notification subscription event payload is inconsistent".into());
    }
    Ok(())
}
