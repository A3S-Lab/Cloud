use super::{Notification, NotificationScope, NotificationSeverity};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, NotificationId, OrganizationId, PrincipalId,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

const OUTBOUND_NOTIFICATION_SCHEMA: &str = "a3s.cloud.notification-delivery.v1";
const MAXIMUM_OUTBOUND_PAYLOAD_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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

/// Immutable, provider-neutral delivery input derived from one in-app notification.
///
/// The target revision is an opaque reference owned by the future subscription/connection
/// boundary. Endpoints and credentials are deliberately absent from this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundNotificationDelivery {
    id: Uuid,
    channel: OutboundNotificationChannel,
    target_revision_id: Uuid,
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
        target_revision_id: Uuid,
    ) -> Result<Self, String> {
        notification.validate()?;
        if target_revision_id.is_nil() {
            return Err("outbound notification target revision must not be nil".into());
        }
        let id = delivery_id(notification.id, channel, target_revision_id);
        let delivery = Self {
            id,
            channel,
            target_revision_id,
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

    pub const fn channel(&self) -> OutboundNotificationChannel {
        self.channel
    }

    pub const fn target_revision_id(&self) -> Uuid {
        self.target_revision_id
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
            || self.target_revision_id.is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.notification_id.as_uuid().is_nil()
            || self.recipient_principal_id.as_uuid().is_nil()
            || self.source_event_id.is_nil()
            || self.correlation_id.is_nil()
            || self.id != delivery_id(self.notification_id, self.channel, self.target_revision_id)
        {
            return Err("outbound notification delivery identity is invalid".into());
        }
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
                target_revision_id: self.target_revision_id,
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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OutboundNotificationPayload<'a> {
    schema: &'static str,
    delivery_id: Uuid,
    channel: OutboundNotificationChannel,
    target_revision_id: Uuid,
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

fn delivery_id(
    notification_id: NotificationId,
    channel: OutboundNotificationChannel,
    target_revision_id: Uuid,
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

    #[test]
    fn delivery_identity_is_stable_per_notification_channel_and_target_revision() {
        let notification = notification();
        let target_revision_id = Uuid::now_v7();
        let first = OutboundNotificationDelivery::from_notification(
            &notification,
            OutboundNotificationChannel::SignedWebhook,
            target_revision_id,
        )
        .expect("delivery");
        let replay = OutboundNotificationDelivery::from_notification(
            &notification,
            OutboundNotificationChannel::SignedWebhook,
            target_revision_id,
        )
        .expect("delivery replay");
        let another_channel = OutboundNotificationDelivery::from_notification(
            &notification,
            OutboundNotificationChannel::SlackCompatible,
            target_revision_id,
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
        assert!(payload.get("readAt").is_none());
        assert!(payload.get("endpoint").is_none());
        assert!(payload.get("credential").is_none());
    }

    #[test]
    fn nil_or_tampered_target_identity_fails_closed() {
        let notification = notification();
        assert!(OutboundNotificationDelivery::from_notification(
            &notification,
            OutboundNotificationChannel::SignedWebhook,
            Uuid::nil(),
        )
        .is_err());
        let mut delivery = OutboundNotificationDelivery::from_notification(
            &notification,
            OutboundNotificationChannel::SignedWebhook,
            Uuid::now_v7(),
        )
        .expect("delivery");
        delivery.target_revision_id = Uuid::now_v7();
        assert!(delivery.validate().is_err());
    }
}
