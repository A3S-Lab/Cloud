use super::{Notification, NotificationScope, NotificationSeverity};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, ConnectorProfileId, ConnectorRevisionId, EnvironmentId, NotificationId,
    OrganizationId, PrincipalId, ProjectId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const OUTBOUND_NOTIFICATION_SCHEMA: &str = "a3s.cloud.notification-delivery.v1";
pub const OUTBOUND_NOTIFICATION_EVENT_KEY: &str = "notification.delivery.requested";
const MAXIMUM_OUTBOUND_PAYLOAD_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboundNotificationChannel {
    SignedWebhook,
    SlackCompatible,
    Smtp,
}

impl OutboundNotificationChannel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SignedWebhook => "signed_webhook",
            Self::SlackCompatible => "slack_compatible",
            Self::Smtp => "smtp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutboundNotificationConnectorTarget {
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
}

impl OutboundNotificationConnectorTarget {
    pub fn new(
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
    ) -> Result<Self, String> {
        let target = Self {
            project_id,
            environment_id,
            profile_id,
            revision_id,
        };
        target.validate()?;
        Ok(target)
    }

    pub fn validate(self) -> Result<(), String> {
        if self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.profile_id.as_uuid().is_nil()
            || self.revision_id.as_uuid().is_nil()
        {
            return Err("outbound notification Connector target must be exact and non-nil".into());
        }
        Ok(())
    }
}

/// Immutable, provider-neutral delivery input derived from one in-app notification.
///
/// The exact target is an opaque reference owned by the subscription/Connector boundary.
/// Endpoints and credentials are deliberately absent from this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundNotificationDelivery {
    id: Uuid,
    channel: OutboundNotificationChannel,
    target: OutboundNotificationConnectorTarget,
    organization_id: OrganizationId,
    notification_id: NotificationId,
    recipient_principal_id: PrincipalId,
    source_event_id: Uuid,
    source_event_key: String,
    correlation_id: Uuid,
    severity: NotificationSeverity,
    title: String,
    body: String,
    scope: NotificationScope,
    occurred_at: DateTime<Utc>,
}

impl OutboundNotificationDelivery {
    pub fn from_notification(
        notification: &Notification,
        channel: OutboundNotificationChannel,
        target: OutboundNotificationConnectorTarget,
    ) -> Result<Self, String> {
        notification.validate()?;
        target.validate()?;
        let id = delivery_id(notification.id, channel, target.revision_id);
        let delivery = Self {
            id,
            channel,
            target,
            organization_id: notification.organization_id,
            notification_id: notification.id,
            recipient_principal_id: notification.recipient_principal_id,
            source_event_id: notification.source_event_id,
            source_event_key: notification.source_event_key.clone(),
            correlation_id: notification.correlation_id,
            severity: notification.severity,
            title: notification.title.clone(),
            body: notification.body.clone(),
            scope: notification.scope,
            occurred_at: notification.occurred_at,
        };
        delivery.validate()?;
        Ok(delivery)
    }

    pub const fn id(&self) -> Uuid {
        self.id
    }

    pub fn requested_event_id(&self) -> Uuid {
        Uuid::new_v5(&self.id, b"notification-delivery-requested:v1")
    }

    pub fn requested_event(&self) -> Result<DomainEventEnvelope, String> {
        self.validate()?;
        Ok(DomainEventEnvelope {
            event_id: self.requested_event_id(),
            event_key: OUTBOUND_NOTIFICATION_EVENT_KEY.into(),
            schema_version: 1,
            organization_id: self.organization_id.as_uuid(),
            aggregate_id: self.id,
            aggregate_version: 1,
            occurred_at: self.occurred_at,
            correlation_id: self.correlation_id,
            causation_id: Some(self.source_event_id),
            payload: self.canonical_payload_value()?,
        })
    }

    pub const fn channel(&self) -> OutboundNotificationChannel {
        self.channel
    }

    pub const fn target_revision_id(&self) -> ConnectorRevisionId {
        self.target.revision_id
    }

    pub const fn target(&self) -> OutboundNotificationConnectorTarget {
        self.target
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn notification_id(&self) -> NotificationId {
        self.notification_id
    }

    pub const fn recipient_principal_id(&self) -> PrincipalId {
        self.recipient_principal_id
    }

    pub const fn source_event_id(&self) -> Uuid {
        self.source_event_id
    }

    pub fn source_event_key(&self) -> &str {
        &self.source_event_key
    }

    pub const fn correlation_id(&self) -> Uuid {
        self.correlation_id
    }

    pub const fn scope(&self) -> NotificationScope {
        self.scope
    }

    pub const fn occurred_at(&self) -> DateTime<Utc> {
        self.occurred_at
    }

    pub const fn severity(&self) -> NotificationSeverity {
        self.severity
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.notification_id.as_uuid().is_nil()
            || self.recipient_principal_id.as_uuid().is_nil()
            || self.source_event_id.is_nil()
            || self.correlation_id.is_nil()
            || self.id != delivery_id(self.notification_id, self.channel, self.target.revision_id)
        {
            return Err("outbound notification delivery identity is invalid".into());
        }
        self.target.validate()?;
        if self.source_event_key.is_empty()
            || self.source_event_key.len() > 255
            || self.title.is_empty()
            || self.body.is_empty()
        {
            return Err("outbound notification delivery content is invalid".into());
        }
        self.canonical_payload().map(|_| ())
    }

    pub fn canonical_payload(&self) -> Result<Vec<u8>, String> {
        canonical_json_bounded(
            &OutboundNotificationPayload {
                schema: OUTBOUND_NOTIFICATION_SCHEMA,
                delivery_id: self.id,
                channel: self.channel,
                project_id: self.target.project_id,
                environment_id: self.target.environment_id,
                target_profile_id: self.target.profile_id,
                target_revision_id: self.target.revision_id,
                organization_id: self.organization_id,
                notification_id: self.notification_id,
                recipient_principal_id: self.recipient_principal_id,
                source_event_id: self.source_event_id,
                source_event_key: &self.source_event_key,
                correlation_id: self.correlation_id,
                severity: self.severity,
                title: &self.title,
                body: &self.body,
                scope: self.scope,
                occurred_at: self.occurred_at,
            },
            MAXIMUM_OUTBOUND_PAYLOAD_BYTES,
            "outbound notification payload",
        )
    }

    pub fn canonical_payload_value(&self) -> Result<serde_json::Value, String> {
        serde_json::from_slice(&self.canonical_payload()?)
            .map_err(|_| "canonical outbound notification payload is invalid JSON".into())
    }

    pub fn from_payload(payload: &serde_json::Value) -> Result<Self, String> {
        let encoded = serde_json::to_vec(payload)
            .map_err(|_| "outbound notification payload is not serializable".to_owned())?;
        if encoded.len() > MAXIMUM_OUTBOUND_PAYLOAD_BYTES {
            return Err("outbound notification payload exceeds its byte limit".into());
        }
        let decoded: OwnedOutboundNotificationPayload = serde_json::from_slice(&encoded)
            .map_err(|_| "outbound notification payload shape is invalid".to_owned())?;
        if decoded.schema != OUTBOUND_NOTIFICATION_SCHEMA {
            return Err("outbound notification payload schema is unsupported".into());
        }
        let delivery = Self {
            id: decoded.delivery_id,
            channel: decoded.channel,
            target: OutboundNotificationConnectorTarget {
                project_id: decoded.project_id,
                environment_id: decoded.environment_id,
                profile_id: decoded.target_profile_id,
                revision_id: decoded.target_revision_id,
            },
            organization_id: decoded.organization_id,
            notification_id: decoded.notification_id,
            recipient_principal_id: decoded.recipient_principal_id,
            source_event_id: decoded.source_event_id,
            source_event_key: decoded.source_event_key,
            correlation_id: decoded.correlation_id,
            severity: decoded.severity,
            title: decoded.title,
            body: decoded.body,
            scope: decoded.scope,
            occurred_at: decoded.occurred_at,
        };
        delivery.validate()?;
        if delivery.canonical_payload_value()? != *payload {
            return Err("outbound notification payload is not canonical".into());
        }
        Ok(delivery)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OutboundNotificationPayload<'a> {
    schema: &'static str,
    delivery_id: Uuid,
    channel: OutboundNotificationChannel,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    target_profile_id: ConnectorProfileId,
    target_revision_id: ConnectorRevisionId,
    organization_id: OrganizationId,
    notification_id: NotificationId,
    recipient_principal_id: PrincipalId,
    source_event_id: Uuid,
    source_event_key: &'a str,
    correlation_id: Uuid,
    severity: NotificationSeverity,
    title: &'a str,
    body: &'a str,
    scope: NotificationScope,
    occurred_at: DateTime<Utc>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnedOutboundNotificationPayload {
    schema: String,
    delivery_id: Uuid,
    channel: OutboundNotificationChannel,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    target_profile_id: ConnectorProfileId,
    target_revision_id: ConnectorRevisionId,
    organization_id: OrganizationId,
    notification_id: NotificationId,
    recipient_principal_id: PrincipalId,
    source_event_id: Uuid,
    source_event_key: String,
    correlation_id: Uuid,
    severity: NotificationSeverity,
    title: String,
    body: String,
    scope: NotificationScope,
    occurred_at: DateTime<Utc>,
}

fn delivery_id(
    notification_id: NotificationId,
    channel: OutboundNotificationChannel,
    target_revision_id: ConnectorRevisionId,
) -> Uuid {
    Uuid::new_v5(
        &notification_id.as_uuid(),
        format!("{}:{target_revision_id}", channel.as_str()).as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::notifications::Notification;

    fn notification() -> Notification {
        let now = Utc::now();
        Notification::project(
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
        .expect("notification")
    }

    fn target(revision_id: ConnectorRevisionId) -> OutboundNotificationConnectorTarget {
        OutboundNotificationConnectorTarget::new(
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            revision_id,
        )
        .expect("target")
    }

    #[test]
    fn delivery_identity_is_stable_per_notification_channel_and_target_revision() {
        let notification = notification();
        let target_revision_id = ConnectorRevisionId::new();
        let first = OutboundNotificationDelivery::from_notification(
            &notification,
            OutboundNotificationChannel::SignedWebhook,
            target(target_revision_id),
        )
        .expect("delivery");
        let replay = OutboundNotificationDelivery::from_notification(
            &notification,
            OutboundNotificationChannel::SignedWebhook,
            first.target(),
        )
        .expect("delivery replay");
        let another_channel = OutboundNotificationDelivery::from_notification(
            &notification,
            OutboundNotificationChannel::SlackCompatible,
            first.target(),
        )
        .expect("Slack-compatible delivery");
        assert_eq!(first, replay);
        assert_ne!(first.id, another_channel.id);

        let payload: serde_json::Value =
            serde_json::from_slice(&first.canonical_payload().expect("canonical payload"))
                .expect("payload JSON");
        assert_eq!(
            payload["schema"],
            serde_json::json!(OUTBOUND_NOTIFICATION_SCHEMA)
        );
        assert_eq!(payload["deliveryId"], serde_json::json!(first.id));
        assert_eq!(
            OutboundNotificationDelivery::from_payload(&payload),
            Ok(first.clone())
        );
        assert!(payload.get("readAt").is_none());
        assert!(payload.get("endpoint").is_none());
        assert!(payload.get("credential").is_none());
        let event = first.requested_event().expect("requested event");
        assert_eq!(event.event_id, first.requested_event_id());
        assert_eq!(event.event_key, OUTBOUND_NOTIFICATION_EVENT_KEY);
        assert_eq!(event.aggregate_id, first.id());
        assert_eq!(event.causation_id, Some(first.source_event_id()));
        assert_eq!(event.payload, payload);
    }

    #[test]
    fn nil_or_tampered_target_identity_fails_closed() {
        let notification = notification();
        assert!(OutboundNotificationDelivery::from_notification(
            &notification,
            OutboundNotificationChannel::SignedWebhook,
            OutboundNotificationConnectorTarget {
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
                profile_id: ConnectorProfileId::new(),
                revision_id: ConnectorRevisionId::from_uuid(Uuid::nil()),
            },
        )
        .is_err());
        let mut delivery = OutboundNotificationDelivery::from_notification(
            &notification,
            OutboundNotificationChannel::SignedWebhook,
            target(ConnectorRevisionId::new()),
        )
        .expect("delivery");
        delivery.target.revision_id = ConnectorRevisionId::new();
        assert!(delivery.validate().is_err());
    }
}
