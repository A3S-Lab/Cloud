use super::{
    Notification, NotificationSeverity, OutboundNotificationChannel,
    OutboundNotificationConnectorTarget,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId,
    NotificationSubscriptionId, OrganizationId, PrincipalId, ProjectId, Sha256Digest,
};
use a3s_acl::builder::{string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA: &str =
    "cloud.notification.outbound-subscription.v1";
pub const OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES: usize = 16 * 1024;
const OUTBOUND_NOTIFICATION_SUBSCRIPTION_BLOCK: &str = "notification_outbound_subscription";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutboundNotificationSubscriptionSpec {
    pub channel: OutboundNotificationChannel,
    pub minimum_severity: NotificationSeverity,
    pub target: OutboundNotificationConnectorTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutboundNotificationSubscriptionDefinition {
    spec: OutboundNotificationSubscriptionSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl OutboundNotificationSubscriptionDefinition {
    pub fn from_spec(spec: OutboundNotificationSubscriptionSpec) -> Result<Self, String> {
        validate_spec(spec)?;
        let document = definition_document(spec);
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES {
            return Err("outbound notification subscription ACL exceeds its byte limit".into());
        }
        let reparsed = parse_acl(&canonical_acl).map_err(|error| {
            format!("generated outbound notification subscription ACL is invalid: {error}")
        })?;
        let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
            format!("outbound notification subscription ACL is not canonicalizable: {error}")
        })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES {
            return Err("outbound notification subscription ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err(
                "outbound notification subscription ACL contains a bare carriage return".into(),
            );
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized).map_err(|error| {
            format!("outbound notification subscription ACL is invalid: {error}")
        })?;
        let definition = Self::from_spec(parse_definition(&document)?)?;
        if definition.canonical_acl != normalized {
            return Err("outbound notification subscription ACL is not canonical".into());
        }
        Ok(definition)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let definition = Self::parse_acl(source)?;
        if definition.digest.as_str() != stored_digest {
            return Err(
                "stored outbound notification subscription ACL and digest do not match".into(),
            );
        }
        Ok(definition)
    }

    pub const fn spec(&self) -> OutboundNotificationSubscriptionSpec {
        self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn matches(&self, notification: &Notification) -> bool {
        severity_rank(notification.severity) >= severity_rank(self.spec.minimum_severity)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundNotificationSubscription {
    pub organization_id: OrganizationId,
    pub id: NotificationSubscriptionId,
    pub recipient_principal_id: PrincipalId,
    pub definition: OutboundNotificationSubscriptionDefinition,
    pub aggregate_version: u64,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl OutboundNotificationSubscription {
    pub fn create(
        organization_id: OrganizationId,
        id: NotificationSubscriptionId,
        recipient_principal_id: PrincipalId,
        definition: OutboundNotificationSubscriptionDefinition,
        actor_principal_id: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if recipient_principal_id != actor_principal_id {
            return Err(
                "outbound notification subscription must be owned by its exact recipient".into(),
            );
        }
        Self::restore(
            organization_id,
            id,
            recipient_principal_id,
            definition,
            1,
            actor_principal_id,
            created_at,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        id: NotificationSubscriptionId,
        recipient_principal_id: PrincipalId,
        definition: OutboundNotificationSubscriptionDefinition,
        aggregate_version: u64,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
        revoked_at: Option<DateTime<Utc>>,
    ) -> Result<Self, String> {
        let subscription = Self {
            organization_id,
            id,
            recipient_principal_id,
            definition,
            aggregate_version,
            created_by,
            created_at: canonical_timestamp(created_at),
            revoked_at: revoked_at.map(canonical_timestamp),
        };
        subscription.validate()?;
        Ok(subscription)
    }

    pub fn revoke(
        &self,
        expected_version: u64,
        actor_principal_id: PrincipalId,
        revoked_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        self.validate()?;
        if !self.is_active()
            || expected_version != self.aggregate_version
            || actor_principal_id != self.recipient_principal_id
        {
            return Err(
                "outbound notification subscription revoke is stale or unauthorized".into(),
            );
        }
        Self::restore(
            self.organization_id,
            self.id,
            self.recipient_principal_id,
            self.definition.clone(),
            self.aggregate_version + 1,
            self.created_by,
            self.created_at,
            Some(canonical_timestamp(revoked_at).max(self.created_at)),
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_spec(self.definition.spec)?;
        if self.organization_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.recipient_principal_id.as_uuid().is_nil()
            || self.created_by != self.recipient_principal_id
            || self.created_at != canonical_timestamp(self.created_at)
            || self.revoked_at != self.revoked_at.map(canonical_timestamp)
        {
            return Err("outbound notification subscription identity or time is invalid".into());
        }
        match (self.aggregate_version, self.revoked_at) {
            (1, None) => Ok(()),
            (2, Some(revoked_at)) if revoked_at >= self.created_at => Ok(()),
            _ => Err("outbound notification subscription lifecycle is invalid".into()),
        }
    }

    pub const fn is_active(&self) -> bool {
        self.aggregate_version == 1 && self.revoked_at.is_none()
    }

    pub fn matches(&self, notification: &Notification) -> bool {
        self.is_active()
            && self.organization_id == notification.organization_id
            && self.recipient_principal_id == notification.recipient_principal_id
            && self.definition.matches(notification)
    }
}

fn validate_spec(spec: OutboundNotificationSubscriptionSpec) -> Result<(), String> {
    spec.target.validate()?;
    if spec.channel == OutboundNotificationChannel::Smtp {
        return Err("SMTP outbound notification subscriptions are unavailable".into());
    }
    Ok(())
}

fn severity_rank(severity: NotificationSeverity) -> u8 {
    match severity {
        NotificationSeverity::Information => 1,
        NotificationSeverity::Warning => 2,
        NotificationSeverity::Critical => 3,
    }
}

fn definition_document(spec: OutboundNotificationSubscriptionSpec) -> Document {
    Document {
        blocks: vec![BlockBuilder::new(OUTBOUND_NOTIFICATION_SUBSCRIPTION_BLOCK)
            .attr("schema", string(OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA))
            .attr("channel", string(spec.channel.as_str()))
            .attr("minimum_severity", string(spec.minimum_severity.as_str()))
            .attr(
                "connector_project_id",
                string(&spec.target.project_id.to_string()),
            )
            .attr(
                "connector_environment_id",
                string(&spec.target.environment_id.to_string()),
            )
            .attr(
                "connector_profile_id",
                string(&spec.target.profile_id.to_string()),
            )
            .attr(
                "connector_revision_id",
                string(&spec.target.revision_id.to_string()),
            )
            .build()],
    }
}

fn parse_definition(document: &Document) -> Result<OutboundNotificationSubscriptionSpec, String> {
    if document.blocks.len() != 1 {
        return Err("outbound notification subscription must contain one top-level block".into());
    }
    let root = &document.blocks[0];
    let attributes = [
        "schema",
        "channel",
        "minimum_severity",
        "connector_project_id",
        "connector_environment_id",
        "connector_profile_id",
        "connector_revision_id",
    ];
    if root.name != OUTBOUND_NOTIFICATION_SUBSCRIPTION_BLOCK
        || !root.labels.is_empty()
        || !root.blocks.is_empty()
        || root.attributes.len() != attributes.len()
        || root
            .attributes
            .keys()
            .any(|key| !attributes.contains(&key.as_str()))
    {
        return Err("outbound notification subscription root shape is invalid".into());
    }
    if required_string(root, "schema")? != OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA {
        return Err("outbound notification subscription schema is unsupported".into());
    }
    let target = OutboundNotificationConnectorTarget::new(
        ProjectId::from_uuid(parse_id(root, "connector_project_id")?),
        EnvironmentId::from_uuid(parse_id(root, "connector_environment_id")?),
        ConnectorProfileId::from_uuid(parse_id(root, "connector_profile_id")?),
        ConnectorRevisionId::from_uuid(parse_id(root, "connector_revision_id")?),
    )?;
    Ok(OutboundNotificationSubscriptionSpec {
        channel: OutboundNotificationChannel::parse(&required_string(root, "channel")?)?,
        minimum_severity: NotificationSeverity::parse(&required_string(root, "minimum_severity")?)?,
        target,
    })
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("outbound notification subscription field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| {
            format!("outbound notification subscription field {name:?} must be a string")
        })
}

fn parse_id(block: &Block, name: &str) -> Result<Uuid, String> {
    Uuid::parse_str(&required_string(block, name)?)
        .ok()
        .filter(|id| !id.is_nil())
        .ok_or_else(|| format!("outbound notification subscription field {name:?} is invalid"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        ConnectorProfileId, ConnectorRevisionId, EnvironmentId, ProjectId,
    };

    fn definition() -> OutboundNotificationSubscriptionDefinition {
        OutboundNotificationSubscriptionDefinition::from_spec(
            OutboundNotificationSubscriptionSpec {
                channel: OutboundNotificationChannel::SignedWebhook,
                minimum_severity: NotificationSeverity::Warning,
                target: OutboundNotificationConnectorTarget::new(
                    ProjectId::new(),
                    EnvironmentId::new(),
                    ConnectorProfileId::new(),
                    ConnectorRevisionId::new(),
                )
                .expect("target"),
            },
        )
        .expect("definition")
    }

    #[test]
    fn subscription_acl_is_canonical_exact_and_smtp_closed() {
        let definition = definition();
        assert_eq!(
            OutboundNotificationSubscriptionDefinition::parse_acl(definition.canonical_acl())
                .expect("reparse"),
            definition
        );
        assert!(definition.canonical_acl().ends_with('\n'));
        assert!(OutboundNotificationSubscriptionDefinition::parse_acl(
            &definition
                .canonical_acl()
                .replace("  channel", "    channel")
        )
        .is_err());
        let mut smtp = definition.spec();
        smtp.channel = OutboundNotificationChannel::Smtp;
        assert!(OutboundNotificationSubscriptionDefinition::from_spec(smtp).is_err());
    }

    #[test]
    fn subscription_is_personal_immutable_and_only_revocable_once() {
        let recipient = PrincipalId::new();
        let now = canonical_timestamp(Utc::now());
        let subscription = OutboundNotificationSubscription::create(
            OrganizationId::new(),
            NotificationSubscriptionId::new(),
            recipient,
            definition(),
            recipient,
            now,
        )
        .expect("subscription");
        assert!(subscription.is_active());
        let revoked = subscription
            .revoke(1, recipient, now)
            .expect("revoked subscription");
        assert!(!revoked.is_active());
        assert!(revoked.revoke(2, recipient, now).is_err());
        assert!(OutboundNotificationSubscription::create(
            OrganizationId::new(),
            NotificationSubscriptionId::new(),
            PrincipalId::new(),
            definition(),
            recipient,
            now,
        )
        .is_err());
    }
}
