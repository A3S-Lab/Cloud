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
pub const OUTBOUND_NOTIFICATION_SCHEMA_V2: &str = "a3s.cloud.notification-delivery.v2";
pub const OUTBOUND_NOTIFICATION_EVENT_KEY: &str = "notification.delivery.requested";
pub const MAXIMUM_OUTBOUND_NOTIFICATION_DELIVERY_GENERATION: u64 = 1_000;
pub const MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS: u64 = 8;
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

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "signed_webhook" => Ok(Self::SignedWebhook),
            "slack_compatible" => Ok(Self::SlackCompatible),
            "smtp" => Ok(Self::Smtp),
            _ => Err("outbound notification channel is invalid".into()),
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
    schema_version: u32,
    maximum_provider_attempts: u64,
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
        Self::from_notification_contract(
            notification,
            channel,
            target,
            1,
            MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
        )
    }

    pub(super) fn from_notification_contract(
        notification: &Notification,
        channel: OutboundNotificationChannel,
        target: OutboundNotificationConnectorTarget,
        schema_version: u32,
        maximum_provider_attempts: u64,
    ) -> Result<Self, String> {
        notification.validate()?;
        target.validate()?;
        let id = delivery_id(notification.id, channel, target.revision_id);
        let delivery = Self {
            id,
            schema_version,
            maximum_provider_attempts,
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
        Uuid::new_v5(
            &self.id,
            format!("notification-delivery-requested:v{}", self.schema_version).as_bytes(),
        )
    }

    pub fn requested_event(&self) -> Result<DomainEventEnvelope, String> {
        self.validate()?;
        Ok(DomainEventEnvelope {
            event_id: self.requested_event_id(),
            event_key: OUTBOUND_NOTIFICATION_EVENT_KEY.into(),
            schema_version: self.schema_version,
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

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub const fn maximum_provider_attempts(&self) -> u64 {
        self.maximum_provider_attempts
    }

    pub const fn schema(&self) -> &'static str {
        if self.schema_version == 1 {
            OUTBOUND_NOTIFICATION_SCHEMA
        } else {
            OUTBOUND_NOTIFICATION_SCHEMA_V2
        }
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
            || !matches!(self.schema_version, 1 | 2)
            || self.maximum_provider_attempts == 0
            || self.maximum_provider_attempts > MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS
            || self.schema_version == 1
                && self.maximum_provider_attempts != MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS
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
        if self.schema_version == 1 {
            return canonical_json_bounded(
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
            );
        }
        canonical_json_bounded(
            &OutboundNotificationPayloadV2 {
                schema: OUTBOUND_NOTIFICATION_SCHEMA_V2,
                maximum_provider_attempts: self.maximum_provider_attempts,
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
        let schema = payload
            .get("schema")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "outbound notification payload schema is unsupported".to_owned())?;
        let decoded = match schema {
            OUTBOUND_NOTIFICATION_SCHEMA => {
                let decoded: OwnedOutboundNotificationPayload = serde_json::from_slice(&encoded)
                    .map_err(|_| "outbound notification payload shape is invalid".to_owned())?;
                DecodedOutboundNotificationPayload::from_v1(decoded)
            }
            OUTBOUND_NOTIFICATION_SCHEMA_V2 => {
                let decoded: OwnedOutboundNotificationPayloadV2 = serde_json::from_slice(&encoded)
                    .map_err(|_| "outbound notification payload shape is invalid".to_owned())?;
                DecodedOutboundNotificationPayload::from_v2(decoded)
            }
            _ => return Err("outbound notification payload schema is unsupported".into()),
        };
        let delivery = Self {
            id: decoded.delivery_id,
            schema_version: decoded.schema_version,
            maximum_provider_attempts: decoded.maximum_provider_attempts,
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

pub fn outbound_notification_attempt_id(
    delivery_id: Uuid,
    generation: u64,
) -> Result<Uuid, String> {
    if delivery_id.is_nil()
        || generation == 0
        || generation > MAXIMUM_OUTBOUND_NOTIFICATION_DELIVERY_GENERATION
    {
        return Err("outbound notification attempt generation is invalid".into());
    }
    Ok(Uuid::new_v5(
        &delivery_id,
        format!("connector-attempt:{generation}").as_bytes(),
    ))
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OutboundNotificationPayloadV2<'a> {
    schema: &'static str,
    maximum_provider_attempts: u64,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnedOutboundNotificationPayloadV2 {
    schema: String,
    maximum_provider_attempts: u64,
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

struct DecodedOutboundNotificationPayload {
    schema_version: u32,
    maximum_provider_attempts: u64,
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

impl DecodedOutboundNotificationPayload {
    fn from_v1(value: OwnedOutboundNotificationPayload) -> Self {
        debug_assert_eq!(value.schema, OUTBOUND_NOTIFICATION_SCHEMA);
        Self {
            schema_version: 1,
            maximum_provider_attempts: MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
            delivery_id: value.delivery_id,
            channel: value.channel,
            project_id: value.project_id,
            environment_id: value.environment_id,
            target_profile_id: value.target_profile_id,
            target_revision_id: value.target_revision_id,
            organization_id: value.organization_id,
            notification_id: value.notification_id,
            recipient_principal_id: value.recipient_principal_id,
            source_event_id: value.source_event_id,
            source_event_key: value.source_event_key,
            correlation_id: value.correlation_id,
            severity: value.severity,
            title: value.title,
            body: value.body,
            scope: value.scope,
            occurred_at: value.occurred_at,
        }
    }

    fn from_v2(value: OwnedOutboundNotificationPayloadV2) -> Self {
        debug_assert_eq!(value.schema, OUTBOUND_NOTIFICATION_SCHEMA_V2);
        Self {
            schema_version: 2,
            maximum_provider_attempts: value.maximum_provider_attempts,
            delivery_id: value.delivery_id,
            channel: value.channel,
            project_id: value.project_id,
            environment_id: value.environment_id,
            target_profile_id: value.target_profile_id,
            target_revision_id: value.target_revision_id,
            organization_id: value.organization_id,
            notification_id: value.notification_id,
            recipient_principal_id: value.recipient_principal_id,
            source_event_id: value.source_event_id,
            source_event_key: value.source_event_key,
            correlation_id: value.correlation_id,
            severity: value.severity,
            title: value.title,
            body: value.body,
            scope: value.scope,
            occurred_at: value.occurred_at,
        }
    }
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
    use crate::modules::notifications::{
        Notification, OutboundNotificationSubscriptionDefinition,
        OutboundNotificationSubscriptionSpec, OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2,
    };

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

    #[test]
    fn version_two_delivery_pins_budget_while_version_one_bytes_remain_unchanged() {
        let notification = notification();
        let target = target(ConnectorRevisionId::new());
        let version_one = OutboundNotificationDelivery::from_notification(
            &notification,
            OutboundNotificationChannel::SignedWebhook,
            target,
        )
        .expect("version one delivery");
        let definition =
            OutboundNotificationSubscriptionDefinition::from_spec_with_provider_attempt_budget(
                OutboundNotificationSubscriptionSpec {
                    channel: OutboundNotificationChannel::SignedWebhook,
                    minimum_severity: NotificationSeverity::Warning,
                    target,
                },
                2,
            )
            .expect("version two subscription");
        assert_eq!(
            definition.definition_schema(),
            OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2
        );
        let version_two = definition
            .delivery_for(&notification)
            .expect("version two delivery");

        assert_eq!(version_one.schema_version(), 1);
        assert_eq!(
            version_one.maximum_provider_attempts(),
            MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS
        );
        let version_one_target = version_one.target();
        let expected_version_one_payload = serde_json::json!({
            "schema": OUTBOUND_NOTIFICATION_SCHEMA,
            "deliveryId": version_one.id(),
            "channel": version_one.channel(),
            "projectId": version_one_target.project_id,
            "environmentId": version_one_target.environment_id,
            "targetProfileId": version_one_target.profile_id,
            "targetRevisionId": version_one_target.revision_id,
            "organizationId": version_one.organization_id(),
            "notificationId": version_one.notification_id(),
            "recipientPrincipalId": version_one.recipient_principal_id(),
            "sourceEventId": version_one.source_event_id(),
            "sourceEventKey": version_one.source_event_key(),
            "correlationId": version_one.correlation_id(),
            "severity": version_one.severity(),
            "title": version_one.title(),
            "body": version_one.body(),
            "scope": version_one.scope(),
            "occurredAt": version_one.occurred_at(),
        });
        assert_eq!(
            version_one.canonical_payload().expect("version one bytes"),
            canonical_json_bounded(
                &expected_version_one_payload,
                MAXIMUM_OUTBOUND_PAYLOAD_BYTES,
                "outbound notification payload",
            )
            .expect("historic version one bytes")
        );
        assert_eq!(
            version_one.requested_event_id(),
            Uuid::new_v5(&version_one.id(), b"notification-delivery-requested:v1")
        );
        assert!(!version_one
            .canonical_payload_value()
            .expect("version one payload")
            .as_object()
            .expect("object")
            .contains_key("maximumProviderAttempts"));
        assert_eq!(version_two.id(), version_one.id());
        assert_ne!(
            version_two.requested_event_id(),
            version_one.requested_event_id()
        );
        assert_eq!(version_two.schema(), OUTBOUND_NOTIFICATION_SCHEMA_V2);
        assert_eq!(version_two.schema_version(), 2);
        assert_eq!(version_two.maximum_provider_attempts(), 2);
        let payload = version_two
            .canonical_payload_value()
            .expect("version two payload");
        assert_eq!(payload["schema"], OUTBOUND_NOTIFICATION_SCHEMA_V2);
        assert_eq!(payload["maximumProviderAttempts"], 2);
        assert_eq!(
            OutboundNotificationDelivery::from_payload(&payload),
            Ok(version_two.clone())
        );
        assert_eq!(
            version_two
                .requested_event()
                .expect("version two event")
                .schema_version,
            2
        );

        let mut invalid = payload;
        invalid["maximumProviderAttempts"] = serde_json::json!(0);
        assert!(OutboundNotificationDelivery::from_payload(&invalid).is_err());
    }
}
