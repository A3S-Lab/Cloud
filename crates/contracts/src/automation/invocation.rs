use super::definition::{
    AutomationRevisionV1, AutomationSubscriptionReferenceV1, AutomationTargetV1,
    AutomationTriggerV1,
};
use super::validation::{
    canonical_json, json_digest, validate_digest, validate_event_key, validate_media_type,
    validate_single_line, validate_timestamp, validate_uuid, MAX_SAFE_INTEGER,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const AUTOMATION_INVOCATION_SCHEMA_V1: &str = "cloud.automation.invocation.v1";
pub const AUTOMATION_INVOCATION_INLINE_MAX_BYTES: usize = 1024 * 1024;
pub const AUTOMATION_INVOCATION_ENVELOPE_MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationInvocationOriginV1 {
    DueTime {
        scheduled_at: DateTime<Utc>,
    },
    Event {
        event_id: Uuid,
        event_key: String,
        event_digest: String,
        observed_at: DateTime<Utc>,
    },
}

impl AutomationInvocationOriginV1 {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::DueTime { scheduled_at } => {
                validate_timestamp("Automation scheduled_at", *scheduled_at)
            }
            Self::Event {
                event_id,
                event_key,
                event_digest,
                observed_at,
            } => {
                validate_uuid("Automation event ID", *event_id)?;
                validate_event_key(event_key)?;
                validate_digest("Automation event digest", event_digest)?;
                validate_timestamp("Automation event observed_at", *observed_at)
            }
        }
    }

    pub const fn is_due_time(&self) -> bool {
        matches!(self, Self::DueTime { .. })
    }

    pub const fn event_id(&self) -> Option<Uuid> {
        match self {
            Self::DueTime { .. } => None,
            Self::Event { event_id, .. } => Some(*event_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationInvocationInputV1 {
    InlineJson {
        value: Value,
        digest: String,
        size_bytes: u64,
    },
    ImmutableObject {
        namespace: String,
        key: String,
        media_type: String,
        digest: String,
        size_bytes: u64,
    },
}

impl AutomationInvocationInputV1 {
    pub fn inline_json(value: Value) -> Result<Self, String> {
        let bytes = canonical_json(&value, AUTOMATION_INVOCATION_INLINE_MAX_BYTES)?;
        let input = Self::InlineJson {
            value,
            digest: json_digest(&bytes),
            size_bytes: bytes.len() as u64,
        };
        input.validate()?;
        Ok(input)
    }

    pub fn immutable_object(
        namespace: impl Into<String>,
        key: impl Into<String>,
        media_type: impl Into<String>,
        digest: impl Into<String>,
        size_bytes: u64,
    ) -> Result<Self, String> {
        let input = Self::ImmutableObject {
            namespace: namespace.into(),
            key: key.into(),
            media_type: media_type.into(),
            digest: digest.into(),
            size_bytes,
        };
        input.validate()?;
        Ok(input)
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::InlineJson {
                value,
                digest,
                size_bytes,
            } => {
                let bytes = canonical_json(value, AUTOMATION_INVOCATION_INLINE_MAX_BYTES)?;
                if *size_bytes != bytes.len() as u64 || *digest != json_digest(&bytes) {
                    return Err("Automation inline input bytes and digest do not match".into());
                }
                Ok(())
            }
            Self::ImmutableObject {
                namespace,
                key,
                media_type,
                digest,
                size_bytes,
            } => {
                validate_dotted_namespace(namespace)?;
                validate_object_key(key)?;
                validate_media_type("Automation immutable-object media type", media_type)?;
                validate_digest("Automation immutable-object digest", digest)?;
                if *size_bytes == 0 || *size_bytes > MAX_SAFE_INTEGER {
                    return Err("Automation immutable-object size is outside its bound".into());
                }
                Ok(())
            }
        }
    }

    pub fn media_type(&self) -> &str {
        match self {
            Self::InlineJson { .. } => "application/json",
            Self::ImmutableObject { media_type, .. } => media_type,
        }
    }

    pub const fn size_bytes(&self) -> u64 {
        match self {
            Self::InlineJson { size_bytes, .. } | Self::ImmutableObject { size_bytes, .. } => {
                *size_bytes
            }
        }
    }

    pub fn digest(&self) -> &str {
        match self {
            Self::InlineJson { digest, .. } | Self::ImmutableObject { digest, .. } => digest,
        }
    }
}

fn validate_dotted_namespace(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || value.split('.').any(|segment| {
            segment.is_empty()
                || segment.starts_with('-')
                || segment.ends_with('-')
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        Err("Automation immutable-object namespace is invalid".into())
    } else {
        Ok(())
    }
}

fn validate_object_key(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 2_048
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains(['\0', '\r', '\n', '\\'])
        || value
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        Err("Automation immutable-object key is invalid".into())
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationInvocationAuthorizationV1 {
    pub policy_digest: String,
    pub grant_snapshot_digest: String,
    pub principal_id: Option<Uuid>,
}

impl AutomationInvocationAuthorizationV1 {
    pub fn validate(&self) -> Result<(), String> {
        validate_digest(
            "Automation invocation authorization policy digest",
            &self.policy_digest,
        )?;
        validate_digest(
            "Automation invocation grant snapshot digest",
            &self.grant_snapshot_digest,
        )?;
        if self.principal_id.is_some_and(|value| value.is_nil()) {
            return Err("Automation invocation principal ID must not be nil".into());
        }
        Ok(())
    }
}

/// One idempotent, exact-release handoff from Automations to the target owner.
/// It carries no provider credentials or mutable "latest" selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationInvocationEnvelopeV1 {
    pub schema: String,
    pub invocation_id: Uuid,
    pub automation_id: Uuid,
    pub automation_revision_id: Uuid,
    pub automation_revision_digest: String,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub target: AutomationTargetV1,
    pub origin: AutomationInvocationOriginV1,
    pub subscription: Option<AutomationSubscriptionReferenceV1>,
    pub deduplication_key: String,
    pub input: AutomationInvocationInputV1,
    pub authorization: AutomationInvocationAuthorizationV1,
    pub requested_at: DateTime<Utc>,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
}

impl AutomationInvocationEnvelopeV1 {
    pub const SCHEMA: &'static str = AUTOMATION_INVOCATION_SCHEMA_V1;

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Automation invocation schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Automation invocation ID", self.invocation_id)?;
        validate_uuid("Automation ID", self.automation_id)?;
        validate_uuid("Automation revision ID", self.automation_revision_id)?;
        validate_digest(
            "Automation invocation revision digest",
            &self.automation_revision_digest,
        )?;
        validate_uuid(
            "Automation invocation organization ID",
            self.organization_id,
        )?;
        validate_uuid("Automation invocation project ID", self.project_id)?;
        validate_uuid("Automation invocation environment ID", self.environment_id)?;
        self.target.validate()?;
        self.origin.validate()?;
        if let Some(subscription) = &self.subscription {
            subscription.validate()?;
        }
        if self.origin.is_due_time() != self.subscription.is_none() {
            return Err(
                "Automation due-time invocations cannot carry a subscription and event invocations must carry one"
                    .into(),
            );
        }
        validate_single_line(
            "Automation invocation deduplication key",
            &self.deduplication_key,
            1_024,
        )?;
        self.input.validate()?;
        self.authorization.validate()?;
        validate_timestamp("Automation invocation requested_at", self.requested_at)?;
        validate_uuid("Automation invocation correlation ID", self.correlation_id)?;
        if self.causation_id.is_some_and(|value| value.is_nil()) {
            return Err("Automation invocation causation ID must not be nil".into());
        }
        canonical_json(self, AUTOMATION_INVOCATION_ENVELOPE_MAX_BYTES)?;
        Ok(())
    }

    pub fn validate_for_revision(&self, revision: &AutomationRevisionV1) -> Result<(), String> {
        self.validate()?;
        revision.validate()?;
        let spec = revision.spec();
        let definition = &spec.definition;
        if self.automation_id != definition.automation_id
            || self.automation_revision_id != spec.revision_id
            || self.automation_revision_digest != revision.digest()
            || self.organization_id != definition.organization_id
            || self.project_id != definition.project_id
            || self.environment_id != definition.environment_id
            || self.target != definition.target
        {
            return Err("Automation invocation does not bind its exact revision target".into());
        }
        if self.subscription.as_ref() != definition.trigger.subscription() {
            return Err("Automation invocation subscription drifted from its trigger".into());
        }
        match (&definition.trigger, &self.origin) {
            (AutomationTriggerV1::Schedule(_), AutomationInvocationOriginV1::DueTime { .. }) => {}
            (AutomationTriggerV1::Webhook(_), AutomationInvocationOriginV1::Event { .. }) => {}
            (
                AutomationTriggerV1::PluginEvent(trigger)
                | AutomationTriggerV1::SourceEvent(trigger),
                AutomationInvocationOriginV1::Event { event_key, .. },
            ) if trigger.event_key == *event_key => {}
            _ => return Err("Automation invocation origin does not match its trigger".into()),
        }
        let expected_key = definition.policy.deduplication.render_key(
            self.automation_id,
            self.automation_revision_id,
            &self.origin,
            self.subscription
                .as_ref()
                .map(|value| value.subscription_id),
        )?;
        if expected_key != self.deduplication_key {
            return Err("Automation invocation deduplication key is not policy-derived".into());
        }
        if self.authorization.policy_digest != definition.authorization.policy_digest {
            return Err("Automation invocation authorization policy drifted".into());
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        Ok(json_digest(&canonical_json(
            self,
            AUTOMATION_INVOCATION_ENVELOPE_MAX_BYTES,
        )?))
    }
}
