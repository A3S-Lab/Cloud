use super::{
    Notification, NotificationSeverity, OutboundNotificationChannel,
    OutboundNotificationConnectorTarget, OutboundNotificationDelivery,
    MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId,
    NotificationSubscriptionId, OrganizationId, PrincipalId, ProjectId, Sha256Digest,
};
use a3s_acl::builder::{number, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::notification::{encode_descending_cursor, parse_descending_cursor};

pub const OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA: &str =
    "cloud.notification.outbound-subscription.v1";
pub const OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2: &str =
    "cloud.notification.outbound-subscription.v2";
pub const OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3: &str =
    "cloud.notification.outbound-subscription.v3";
pub const OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES: usize = 16 * 1024;
pub const MINIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS: u64 = 1;
pub const MAXIMUM_OUTBOUND_NOTIFICATION_SUPPRESSION_DAYS: i64 = 30;
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
    #[serde(default = "default_subscription_schema")]
    definition_schema: String,
    #[serde(default = "default_provider_attempt_budget")]
    maximum_provider_attempts: u64,
    #[serde(default)]
    suppress_before: Option<DateTime<Utc>>,
}

impl OutboundNotificationSubscriptionDefinition {
    pub fn from_spec(spec: OutboundNotificationSubscriptionSpec) -> Result<Self, String> {
        Self::from_versioned_spec(
            spec,
            OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA,
            MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
            None,
        )
    }

    pub fn from_spec_with_provider_attempt_budget(
        spec: OutboundNotificationSubscriptionSpec,
        maximum_provider_attempts: u64,
    ) -> Result<Self, String> {
        Self::from_versioned_spec(
            spec,
            OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2,
            maximum_provider_attempts,
            None,
        )
    }

    pub fn from_spec_with_suppression(
        spec: OutboundNotificationSubscriptionSpec,
        maximum_provider_attempts: u64,
        suppress_before: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::from_versioned_spec(
            spec,
            OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3,
            maximum_provider_attempts,
            Some(suppress_before),
        )
    }

    fn from_versioned_spec(
        spec: OutboundNotificationSubscriptionSpec,
        definition_schema: &str,
        maximum_provider_attempts: u64,
        suppress_before: Option<DateTime<Utc>>,
    ) -> Result<Self, String> {
        validate_spec(spec)?;
        let suppress_before = suppress_before.map(canonical_timestamp);
        validate_versioned_policy(
            definition_schema,
            maximum_provider_attempts,
            suppress_before,
        )?;
        let document = definition_document(
            spec,
            definition_schema,
            maximum_provider_attempts,
            suppress_before,
        );
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
            definition_schema: definition_schema.into(),
            maximum_provider_attempts,
            suppress_before,
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
        let parsed = parse_definition(&document)?;
        let definition = Self::from_versioned_spec(
            parsed.spec,
            &parsed.definition_schema,
            parsed.maximum_provider_attempts,
            parsed.suppress_before,
        )?;
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

    pub fn definition_schema(&self) -> &str {
        &self.definition_schema
    }

    pub fn schema_version(&self) -> u32 {
        match self.definition_schema.as_str() {
            OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA => 1,
            OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2 => 2,
            OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3 => 3,
            _ => unreachable!("validated outbound subscription schema"),
        }
    }

    /// Subscription v3 changes admission only; eligible facts retain the delivery-v2 contract.
    pub fn delivery_schema_version(&self) -> u32 {
        if self.definition_schema == OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA {
            1
        } else {
            2
        }
    }

    pub const fn maximum_provider_attempts(&self) -> u64 {
        self.maximum_provider_attempts
    }

    pub const fn suppress_before(&self) -> Option<DateTime<Utc>> {
        self.suppress_before
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn delivery_for(
        &self,
        notification: &Notification,
    ) -> Result<OutboundNotificationDelivery, String> {
        self.validate()?;
        OutboundNotificationDelivery::from_notification_contract(
            notification,
            self.spec.channel,
            self.spec.target,
            self.delivery_schema_version(),
            self.maximum_provider_attempts,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        let reparsed = Self::parse_acl(&self.canonical_acl)?;
        if &reparsed != self {
            return Err("outbound notification subscription definition is inconsistent".into());
        }
        Ok(())
    }

    pub fn matches(&self, notification: &Notification) -> bool {
        severity_rank(notification.severity) >= severity_rank(self.spec.minimum_severity)
            && self
                .suppress_before
                .is_none_or(|cutoff| notification.occurred_at >= cutoff)
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
        self.definition.validate()?;
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
            (1, None) | (2, Some(_)) => {}
            _ => return Err("outbound notification subscription lifecycle is invalid".into()),
        }
        if self
            .revoked_at
            .is_some_and(|revoked_at| revoked_at < self.created_at)
        {
            return Err("outbound notification subscription lifecycle is invalid".into());
        }
        if let Some(suppress_before) = self.definition.suppress_before() {
            let latest = self
                .created_at
                .checked_add_signed(Duration::days(
                    MAXIMUM_OUTBOUND_NOTIFICATION_SUPPRESSION_DAYS,
                ))
                .ok_or_else(|| {
                    "outbound notification suppression cutoff exceeds the timestamp range"
                        .to_owned()
                })?;
            if suppress_before <= self.created_at || suppress_before > latest {
                return Err(
                    "outbound notification suppression cutoff must be after creation and within 30 days"
                        .into(),
                );
            }
        }
        Ok(())
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutboundNotificationSubscriptionCursor {
    pub created_at: DateTime<Utc>,
    pub subscription_id: NotificationSubscriptionId,
}

impl OutboundNotificationSubscriptionCursor {
    pub fn after(subscription: &OutboundNotificationSubscription) -> Self {
        Self {
            created_at: subscription.created_at,
            subscription_id: subscription.id,
        }
    }

    pub fn encode(self) -> String {
        encode_descending_cursor(self.created_at, self.subscription_id.as_uuid())
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        let (created_at, subscription_id) =
            parse_descending_cursor(value, "outbound notification subscription")?;
        Ok(Self {
            created_at,
            subscription_id: NotificationSubscriptionId::from_uuid(subscription_id),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundNotificationSubscriptionPage {
    pub subscriptions: Vec<OutboundNotificationSubscription>,
    pub next_cursor: Option<String>,
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

fn definition_document(
    spec: OutboundNotificationSubscriptionSpec,
    definition_schema: &str,
    maximum_provider_attempts: u64,
    suppress_before: Option<DateTime<Utc>>,
) -> Document {
    let mut root = BlockBuilder::new(OUTBOUND_NOTIFICATION_SUBSCRIPTION_BLOCK)
        .attr("schema", string(definition_schema))
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
        );
    if matches!(
        definition_schema,
        OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2 | OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3
    ) {
        root = root.attr(
            "maximum_provider_attempts",
            number(maximum_provider_attempts as f64),
        );
    }
    if let Some(suppress_before) = suppress_before {
        root = root.attr(
            "suppress_before",
            string(&suppress_before.to_rfc3339_opts(SecondsFormat::Micros, true)),
        );
    }
    Document {
        blocks: vec![root.build()],
    }
}

struct ParsedSubscriptionDefinition {
    spec: OutboundNotificationSubscriptionSpec,
    definition_schema: String,
    maximum_provider_attempts: u64,
    suppress_before: Option<DateTime<Utc>>,
}

fn parse_definition(document: &Document) -> Result<ParsedSubscriptionDefinition, String> {
    if document.blocks.len() != 1 {
        return Err("outbound notification subscription must contain one top-level block".into());
    }
    let root = &document.blocks[0];
    let common_attributes = [
        "schema",
        "channel",
        "minimum_severity",
        "connector_project_id",
        "connector_environment_id",
        "connector_profile_id",
        "connector_revision_id",
    ];
    let definition_schema = required_string(root, "schema")?;
    let attributes = if definition_schema == OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA {
        common_attributes.to_vec()
    } else if definition_schema == OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2 {
        let mut attributes = common_attributes.to_vec();
        attributes.push("maximum_provider_attempts");
        attributes
    } else if definition_schema == OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3 {
        let mut attributes = common_attributes.to_vec();
        attributes.extend(["maximum_provider_attempts", "suppress_before"]);
        attributes
    } else {
        return Err("outbound notification subscription schema is unsupported".into());
    };
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
    let maximum_provider_attempts =
        if definition_schema == OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA {
            MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS
        } else {
            required_bounded_u64(root, "maximum_provider_attempts")?
        };
    let suppress_before = (definition_schema == OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3)
        .then(|| required_timestamp(root, "suppress_before"))
        .transpose()?;
    validate_versioned_policy(
        &definition_schema,
        maximum_provider_attempts,
        suppress_before,
    )?;
    let target = OutboundNotificationConnectorTarget::new(
        ProjectId::from_uuid(parse_id(root, "connector_project_id")?),
        EnvironmentId::from_uuid(parse_id(root, "connector_environment_id")?),
        ConnectorProfileId::from_uuid(parse_id(root, "connector_profile_id")?),
        ConnectorRevisionId::from_uuid(parse_id(root, "connector_revision_id")?),
    )?;
    Ok(ParsedSubscriptionDefinition {
        spec: OutboundNotificationSubscriptionSpec {
            channel: OutboundNotificationChannel::parse(&required_string(root, "channel")?)?,
            minimum_severity: NotificationSeverity::parse(&required_string(
                root,
                "minimum_severity",
            )?)?,
            target,
        },
        definition_schema,
        maximum_provider_attempts,
        suppress_before,
    })
}

fn validate_versioned_policy(
    definition_schema: &str,
    maximum_provider_attempts: u64,
    suppress_before: Option<DateTime<Utc>>,
) -> Result<(), String> {
    if definition_schema != OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA
        && definition_schema != OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2
        && definition_schema != OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3
    {
        return Err("outbound notification subscription schema is unsupported".into());
    }
    if !(MINIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS
        ..=MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS)
        .contains(&maximum_provider_attempts)
        || definition_schema == OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA
            && maximum_provider_attempts != MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS
    {
        return Err(
            "outbound notification maximum provider attempts must be between 1 and 8".into(),
        );
    }
    match (definition_schema, suppress_before) {
        (OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3, Some(cutoff))
            if cutoff == canonical_timestamp(cutoff) =>
        {
            Ok(())
        }
        (OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3, _) => Err(
            "outbound notification subscription v3 requires a canonical suppression cutoff".into(),
        ),
        (_, None) => Ok(()),
        (_, Some(_)) => {
            Err("outbound notification suppression cutoff requires subscription schema v3".into())
        }
    }
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

fn required_bounded_u64(block: &Block, name: &str) -> Result<u64, String> {
    let Value::Number(value) = required_value(block, name)? else {
        return Err(format!(
            "outbound notification subscription field {name:?} must be an integer"
        ));
    };
    if !value.is_finite()
        || value.fract() != 0.0
        || *value < MINIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS as f64
        || *value > MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS as f64
    {
        return Err(format!(
            "outbound notification subscription field {name:?} must be between 1 and 8"
        ));
    }
    Ok(*value as u64)
}

fn required_timestamp(block: &Block, name: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(&required_string(block, name)?)
        .map_err(|_| {
            format!(
                "outbound notification subscription field {name:?} must be an RFC 3339 timestamp"
            )
        })
        .map(|value| value.with_timezone(&Utc))
}

fn default_subscription_schema() -> String {
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA.into()
}

const fn default_provider_attempt_budget() -> u64 {
    MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        ConnectorProfileId, ConnectorRevisionId, EnvironmentId, ProjectId,
    };

    fn notification(
        organization_id: OrganizationId,
        recipient: PrincipalId,
        occurred_at: DateTime<Utc>,
    ) -> Notification {
        Notification::project(
            organization_id,
            recipient,
            Uuid::now_v7(),
            "workload.health.changed".into(),
            1,
            Uuid::now_v7(),
            1,
            Uuid::now_v7(),
            NotificationSeverity::Critical,
            "Workload unhealthy".into(),
            "The workload health check failed.".into(),
            super::super::NotificationScope::Organization,
            occurred_at,
            occurred_at,
        )
        .expect("notification")
    }

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
        let spec = definition.spec();
        let expected_acl = format!(
            concat!(
                "notification_outbound_subscription {{\n",
                "  channel = \"signed_webhook\"\n",
                "  connector_environment_id = \"{}\"\n",
                "  connector_profile_id = \"{}\"\n",
                "  connector_project_id = \"{}\"\n",
                "  connector_revision_id = \"{}\"\n",
                "  minimum_severity = \"warning\"\n",
                "  schema = \"cloud.notification.outbound-subscription.v1\"\n",
                "}}\n"
            ),
            spec.target.environment_id,
            spec.target.profile_id,
            spec.target.project_id,
            spec.target.revision_id,
        );
        assert_eq!(definition.canonical_acl(), expected_acl);
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
        assert_eq!(
            definition.definition_schema(),
            OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA
        );
        assert_eq!(
            definition.maximum_provider_attempts(),
            MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS
        );
        assert!(!definition
            .canonical_acl()
            .contains("maximum_provider_attempts"));
        assert_eq!(definition.suppress_before(), None);
    }

    #[test]
    fn version_two_acl_pins_a_bounded_provider_attempt_budget() {
        let spec = definition().spec();
        let definition =
            OutboundNotificationSubscriptionDefinition::from_spec_with_provider_attempt_budget(
                spec, 3,
            )
            .expect("version two definition");
        assert_eq!(
            definition.definition_schema(),
            OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2
        );
        assert_eq!(definition.schema_version(), 2);
        assert_eq!(definition.maximum_provider_attempts(), 3);
        assert_eq!(definition.suppress_before(), None);
        assert!(definition
            .canonical_acl()
            .contains("maximum_provider_attempts = 3"));
        assert_eq!(
            OutboundNotificationSubscriptionDefinition::parse_acl(definition.canonical_acl()),
            Ok(definition.clone())
        );
        assert!(
            OutboundNotificationSubscriptionDefinition::from_spec_with_provider_attempt_budget(
                spec, 0
            )
            .is_err()
        );
        assert!(
            OutboundNotificationSubscriptionDefinition::from_spec_with_provider_attempt_budget(
                spec, 9
            )
            .is_err()
        );
        assert!(OutboundNotificationSubscriptionDefinition::parse_acl(
            &definition
                .canonical_acl()
                .replace("  maximum_provider_attempts = 3\n", "")
        )
        .is_err());
        assert!(OutboundNotificationSubscriptionDefinition::parse_acl(
            &definition.canonical_acl().replace(
                "  maximum_provider_attempts = 3",
                "  maximum_provider_attempts = 2.5"
            )
        )
        .is_err());
    }

    #[test]
    fn checked_in_version_two_contract_is_canonical() {
        let source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/c0.3/outbound-notification-subscription-v2.acl"
        ));
        let definition =
            OutboundNotificationSubscriptionDefinition::parse_acl(source).expect("contract");
        assert_eq!(
            definition.definition_schema(),
            OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2
        );
        assert_eq!(definition.maximum_provider_attempts(), 3);
    }

    #[test]
    fn version_three_acl_suppresses_strictly_by_bounded_source_event_time() {
        let organization_id = OrganizationId::new();
        let recipient = PrincipalId::new();
        let created_at = canonical_timestamp(Utc::now());
        let suppress_before = created_at + Duration::days(1);
        let definition = OutboundNotificationSubscriptionDefinition::from_spec_with_suppression(
            definition().spec(),
            3,
            suppress_before,
        )
        .expect("version three definition");
        assert_eq!(
            definition.definition_schema(),
            OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3
        );
        assert_eq!(definition.schema_version(), 3);
        assert_eq!(definition.delivery_schema_version(), 2);
        assert_eq!(definition.maximum_provider_attempts(), 3);
        assert_eq!(definition.suppress_before(), Some(suppress_before));
        assert!(definition.canonical_acl().contains(&format!(
            "suppress_before = \"{}\"",
            suppress_before.to_rfc3339_opts(SecondsFormat::Micros, true)
        )));
        assert_eq!(
            OutboundNotificationSubscriptionDefinition::parse_acl(definition.canonical_acl()),
            Ok(definition.clone())
        );

        let subscription = OutboundNotificationSubscription::create(
            organization_id,
            NotificationSubscriptionId::new(),
            recipient,
            definition.clone(),
            recipient,
            created_at,
        )
        .expect("bounded suppressed subscription");
        assert!(!subscription.matches(&notification(
            organization_id,
            recipient,
            suppress_before - Duration::microseconds(1),
        )));
        let boundary = notification(organization_id, recipient, suppress_before);
        assert!(subscription.matches(&boundary));
        let delivery = definition
            .delivery_for(&boundary)
            .expect("eligible delivery");
        assert_eq!(delivery.schema_version(), 2);
        assert_eq!(delivery.maximum_provider_attempts(), 3);

        for invalid_cutoff in [
            created_at,
            created_at + Duration::days(30) + Duration::microseconds(1),
        ] {
            let invalid = OutboundNotificationSubscriptionDefinition::from_spec_with_suppression(
                definition.spec(),
                3,
                invalid_cutoff,
            )
            .expect("definition is independent from subscription creation time");
            assert!(OutboundNotificationSubscription::create(
                organization_id,
                NotificationSubscriptionId::new(),
                recipient,
                invalid,
                recipient,
                created_at,
            )
            .is_err());
        }
        assert!(OutboundNotificationSubscriptionDefinition::parse_acl(
            &definition
                .canonical_acl()
                .replace("  suppress_before = ", "  unknown_suppression = ")
        )
        .is_err());
    }

    #[test]
    fn checked_in_version_three_contract_is_canonical() {
        let source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/c0.3/outbound-notification-subscription-v3.acl"
        ));
        let definition =
            OutboundNotificationSubscriptionDefinition::parse_acl(source).expect("contract");
        assert_eq!(
            definition.definition_schema(),
            OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3
        );
        assert_eq!(definition.maximum_provider_attempts(), 3);
        assert_eq!(
            definition
                .suppress_before()
                .map(|value| value.to_rfc3339_opts(SecondsFormat::Micros, true)),
            Some("2026-09-01T00:00:00.000000Z".into())
        );
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
