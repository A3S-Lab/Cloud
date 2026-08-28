use super::{Notification, NotificationScope, NotificationSeverity};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, ConnectorProfileId, ConnectorRevisionId, EnvironmentId, NotificationId,
    OrganizationId, PrincipalId, ProjectId, RecipientContactId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const OUTBOUND_NOTIFICATION_SCHEMA: &str = "a3s.cloud.notification-delivery.v1";
pub const OUTBOUND_NOTIFICATION_SCHEMA_V2: &str = "a3s.cloud.notification-delivery.v2";
pub const OUTBOUND_NOTIFICATION_SCHEMA_V3: &str = "a3s.cloud.notification-delivery.v3";
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboundNotificationTarget {
    Connector(OutboundNotificationConnectorTarget),
    RecipientContact(RecipientContactId),
}

impl OutboundNotificationTarget {
    pub fn recipient_contact(recipient_contact_id: RecipientContactId) -> Result<Self, String> {
        let target = Self::RecipientContact(recipient_contact_id);
        target.validate()?;
        Ok(target)
    }

    pub fn validate(self) -> Result<(), String> {
        match self {
            Self::Connector(target) => target.validate(),
            Self::RecipientContact(contact_id) if !contact_id.as_uuid().is_nil() => Ok(()),
            Self::RecipientContact(_) => {
                Err("outbound notification recipient contact target must be non-nil".into())
            }
        }
    }

    pub fn validate_for_channel(self, channel: OutboundNotificationChannel) -> Result<(), String> {
        self.validate()?;
        if matches!(
            (channel, self),
            (
                OutboundNotificationChannel::SignedWebhook
                    | OutboundNotificationChannel::SlackCompatible,
                Self::Connector(_)
            ) | (OutboundNotificationChannel::Smtp, Self::RecipientContact(_))
        ) {
            Ok(())
        } else {
            Err("outbound notification channel and target authority do not match".into())
        }
    }

    pub const fn connector(self) -> Option<OutboundNotificationConnectorTarget> {
        match self {
            Self::Connector(target) => Some(target),
            Self::RecipientContact(_) => None,
        }
    }

    pub const fn recipient_contact_id(self) -> Option<RecipientContactId> {
        match self {
            Self::Connector(_) => None,
            Self::RecipientContact(contact_id) => Some(contact_id),
        }
    }

    const fn identity_id(self) -> Uuid {
        match self {
            Self::Connector(target) => target.revision_id.as_uuid(),
            Self::RecipientContact(contact_id) => contact_id.as_uuid(),
        }
    }
}

impl From<OutboundNotificationConnectorTarget> for OutboundNotificationTarget {
    fn from(value: OutboundNotificationConnectorTarget) -> Self {
        Self::Connector(value)
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
    target: OutboundNotificationTarget,
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
    pub fn from_notification<T: Into<OutboundNotificationTarget>>(
        notification: &Notification,
        channel: OutboundNotificationChannel,
        target: T,
    ) -> Result<Self, String> {
        Self::from_notification_contract(
            notification,
            channel,
            target.into(),
            1,
            MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
        )
    }

    pub(super) fn from_notification_contract(
        notification: &Notification,
        channel: OutboundNotificationChannel,
        target: OutboundNotificationTarget,
        schema_version: u32,
        maximum_provider_attempts: u64,
    ) -> Result<Self, String> {
        notification.validate()?;
        target.validate_for_channel(channel)?;
        let id = delivery_id(notification.id, channel, target.identity_id());
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
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: self.organization_id.as_uuid(),
            },
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

    pub fn schema(&self) -> &'static str {
        match self.schema_version {
            1 => OUTBOUND_NOTIFICATION_SCHEMA,
            2 => OUTBOUND_NOTIFICATION_SCHEMA_V2,
            3 => OUTBOUND_NOTIFICATION_SCHEMA_V3,
            _ => unreachable!("validated outbound notification delivery schema"),
        }
    }

    pub const fn target_revision_id(&self) -> Option<ConnectorRevisionId> {
        match self.target {
            OutboundNotificationTarget::Connector(target) => Some(target.revision_id),
            OutboundNotificationTarget::RecipientContact(_) => None,
        }
    }

    pub const fn target(&self) -> OutboundNotificationTarget {
        self.target
    }

    pub const fn connector_target(&self) -> Option<OutboundNotificationConnectorTarget> {
        self.target.connector()
    }

    pub const fn recipient_contact_id(&self) -> Option<RecipientContactId> {
        self.target.recipient_contact_id()
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
            || self.id
                != delivery_id(
                    self.notification_id,
                    self.channel,
                    self.target.identity_id(),
                )
            || !matches!(self.schema_version, 1..=3)
            || self.maximum_provider_attempts == 0
            || self.maximum_provider_attempts > MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS
            || self.schema_version == 1
                && self.maximum_provider_attempts != MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS
        {
            return Err("outbound notification delivery identity is invalid".into());
        }
        self.target.validate_for_channel(self.channel)?;
        if self.schema_version <= 2 && self.target.connector().is_none()
            || self.schema_version == 3 && self.target.recipient_contact_id().is_none()
        {
            return Err("outbound notification delivery schema and target do not match".into());
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
        if self.schema_version == 1 {
            let target = self.target.connector().ok_or_else(|| {
                "outbound notification delivery v1 requires a Connector target".to_owned()
            })?;
            return canonical_json_bounded(
                &OutboundNotificationPayload {
                    schema: OUTBOUND_NOTIFICATION_SCHEMA,
                    delivery_id: self.id,
                    channel: self.channel,
                    project_id: target.project_id,
                    environment_id: target.environment_id,
                    target_profile_id: target.profile_id,
                    target_revision_id: target.revision_id,
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
        if self.schema_version == 2 {
            let target = self.target.connector().ok_or_else(|| {
                "outbound notification delivery v2 requires a Connector target".to_owned()
            })?;
            return canonical_json_bounded(
                &OutboundNotificationPayloadV2 {
                    schema: OUTBOUND_NOTIFICATION_SCHEMA_V2,
                    maximum_provider_attempts: self.maximum_provider_attempts,
                    delivery_id: self.id,
                    channel: self.channel,
                    project_id: target.project_id,
                    environment_id: target.environment_id,
                    target_profile_id: target.profile_id,
                    target_revision_id: target.revision_id,
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
            &OutboundNotificationPayloadV3 {
                schema: OUTBOUND_NOTIFICATION_SCHEMA_V3,
                maximum_provider_attempts: self.maximum_provider_attempts,
                delivery_id: self.id,
                channel: self.channel,
                recipient_contact_id: self.target.recipient_contact_id().ok_or_else(|| {
                    "outbound notification delivery v3 requires a recipient contact target"
                        .to_owned()
                })?,
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
            OUTBOUND_NOTIFICATION_SCHEMA_V3 => {
                let decoded: OwnedOutboundNotificationPayloadV3 = serde_json::from_slice(&encoded)
                    .map_err(|_| "outbound notification payload shape is invalid".to_owned())?;
                DecodedOutboundNotificationPayload::from_v3(decoded)
            }
            _ => return Err("outbound notification payload schema is unsupported".into()),
        };
        let delivery = Self {
            id: decoded.delivery_id,
            schema_version: decoded.schema_version,
            maximum_provider_attempts: decoded.maximum_provider_attempts,
            channel: decoded.channel,
            target: decoded.target,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OutboundNotificationPayloadV3<'a> {
    schema: &'static str,
    maximum_provider_attempts: u64,
    delivery_id: Uuid,
    channel: OutboundNotificationChannel,
    recipient_contact_id: RecipientContactId,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnedOutboundNotificationPayloadV3 {
    schema: String,
    maximum_provider_attempts: u64,
    delivery_id: Uuid,
    channel: OutboundNotificationChannel,
    recipient_contact_id: RecipientContactId,
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
    target: OutboundNotificationTarget,
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
            target: OutboundNotificationTarget::Connector(OutboundNotificationConnectorTarget {
                project_id: value.project_id,
                environment_id: value.environment_id,
                profile_id: value.target_profile_id,
                revision_id: value.target_revision_id,
            }),
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
            target: OutboundNotificationTarget::Connector(OutboundNotificationConnectorTarget {
                project_id: value.project_id,
                environment_id: value.environment_id,
                profile_id: value.target_profile_id,
                revision_id: value.target_revision_id,
            }),
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

    fn from_v3(value: OwnedOutboundNotificationPayloadV3) -> Self {
        debug_assert_eq!(value.schema, OUTBOUND_NOTIFICATION_SCHEMA_V3);
        Self {
            schema_version: 3,
            maximum_provider_attempts: value.maximum_provider_attempts,
            delivery_id: value.delivery_id,
            channel: value.channel,
            target: OutboundNotificationTarget::RecipientContact(value.recipient_contact_id),
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
    target_id: Uuid,
) -> Uuid {
    Uuid::new_v5(
        &notification_id.as_uuid(),
        format!("{}:{target_id}", channel.as_str()).as_bytes(),
    )
}

#[cfg(test)]
#[path = "outbound_delivery_tests.rs"]
mod tests;
