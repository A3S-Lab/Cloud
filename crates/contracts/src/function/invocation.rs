use super::profile::{FunctionEgressClassV1, FunctionProfileV1};
use super::validation::{
    canonical_json, json_digest, validate_digest, validate_dotted_identifier, validate_media_type,
    validate_object_key, validate_single_line, validate_uuid, MAX_SAFE_INTEGER,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const FUNCTION_INVOCATION_SCHEMA_V1: &str = "cloud.function.invocation.v1";
pub const FUNCTION_INVOCATION_INLINE_MAX_BYTES: usize = 1024 * 1024;
pub const FUNCTION_INVOCATION_ENVELOPE_MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionInvocationParentKindV1 {
    Direct,
    Workflow,
    Agent,
    Automation,
}

impl FunctionInvocationParentKindV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Workflow => "workflow",
            Self::Agent => "agent",
            Self::Automation => "automation",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionInvocationParentV1 {
    pub kind: FunctionInvocationParentKindV1,
    pub id: Uuid,
    pub revision_digest: String,
}

impl FunctionInvocationParentV1 {
    fn validate(&self) -> Result<(), String> {
        validate_uuid("Function invocation parent ID", self.id)?;
        validate_digest(
            "Function invocation parent revision digest",
            &self.revision_digest,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionInvocationSlotV1 {
    pub name: String,
    pub attempt: u64,
}

impl FunctionInvocationSlotV1 {
    fn validate(&self) -> Result<(), String> {
        validate_dotted_identifier("Function invocation slot", &self.name, 128)?;
        if self.attempt == 0 || self.attempt > MAX_SAFE_INTEGER {
            return Err("Function invocation attempt must be a positive JSON-safe integer".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionInvocationTargetV1 {
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub profile_digest: String,
}

impl FunctionInvocationTargetV1 {
    fn validate(&self) -> Result<(), String> {
        validate_uuid("Function invocation Asset ID", self.asset_id)?;
        validate_uuid(
            "Function invocation Asset release ID",
            self.asset_release_id,
        )?;
        validate_digest("Function invocation profile digest", &self.profile_digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FunctionInvocationInputV1 {
    InlineJson {
        media_type: String,
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

impl FunctionInvocationInputV1 {
    pub fn inline_json(media_type: impl Into<String>, value: Value) -> Result<Self, String> {
        let media_type = media_type.into();
        let bytes = canonical_json(&value, FUNCTION_INVOCATION_INLINE_MAX_BYTES)?;
        let input = Self::InlineJson {
            media_type,
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

    pub fn media_type(&self) -> &str {
        match self {
            Self::InlineJson { media_type, .. } | Self::ImmutableObject { media_type, .. } => {
                media_type
            }
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

    fn validate(&self) -> Result<(), String> {
        match self {
            Self::InlineJson {
                media_type,
                value,
                digest,
                size_bytes,
            } => {
                if media_type != "application/json" {
                    return Err("inline Function input must use application/json".into());
                }
                let bytes = canonical_json(value, FUNCTION_INVOCATION_INLINE_MAX_BYTES)?;
                if *size_bytes != bytes.len() as u64 || *digest != json_digest(&bytes) {
                    return Err("inline Function input bytes and digest do not match".into());
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
                validate_dotted_identifier("Function immutable-object namespace", namespace, 128)?;
                validate_object_key(key)?;
                validate_media_type("Function immutable-object media type", media_type)?;
                validate_digest("Function immutable-object digest", digest)?;
                if *size_bytes > MAX_SAFE_INTEGER {
                    return Err("Function immutable-object size exceeds JSON-safe bounds".into());
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionInvocationPolicyV1 {
    pub requested_at: DateTime<Utc>,
    pub deadline_at: DateTime<Utc>,
    pub idempotency_key: String,
    pub authorization_digest: String,
    pub egress_class: FunctionEgressClassV1,
}

impl FunctionInvocationPolicyV1 {
    fn validate(&self) -> Result<(), String> {
        if !is_millisecond_timestamp(self.requested_at)
            || !is_millisecond_timestamp(self.deadline_at)
            || self.deadline_at <= self.requested_at
        {
            return Err(
                "Function invocation timestamps must be ordered canonical milliseconds".into(),
            );
        }
        validate_single_line(
            "Function invocation idempotency key",
            &self.idempotency_key,
            255,
        )?;
        validate_digest(
            "Function invocation authorization digest",
            &self.authorization_digest,
        )
    }
}

/// One immutable authority envelope shared by direct, Workflow, Agent, and
/// Automation callers. Resolution delegates it to exactly one profile owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionInvocationAuthorityV1 {
    pub schema: String,
    pub invocation_id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub parent: FunctionInvocationParentV1,
    pub slot: FunctionInvocationSlotV1,
    pub target: FunctionInvocationTargetV1,
    pub input: FunctionInvocationInputV1,
    pub policy: FunctionInvocationPolicyV1,
}

impl FunctionInvocationAuthorityV1 {
    pub const SCHEMA: &'static str = FUNCTION_INVOCATION_SCHEMA_V1;

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Function invocation schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Function invocation ID", self.invocation_id)?;
        validate_uuid("Function invocation organization ID", self.organization_id)?;
        validate_uuid("Function invocation project ID", self.project_id)?;
        validate_uuid("Function invocation environment ID", self.environment_id)?;
        self.parent.validate()?;
        self.slot.validate()?;
        self.target.validate()?;
        self.input.validate()?;
        self.policy.validate()?;
        canonical_json(self, FUNCTION_INVOCATION_ENVELOPE_MAX_BYTES)?;
        Ok(())
    }

    pub fn validate_for_profile(&self, profile: &FunctionProfileV1) -> Result<(), String> {
        self.validate()?;
        let spec = profile.spec();
        if self.organization_id != spec.organization_id
            || self.target.asset_id != spec.asset_id
            || self.target.asset_release_id != spec.asset_release_id
            || self.target.profile_digest != profile.digest()
        {
            return Err("Function invocation target does not match its immutable profile".into());
        }
        if self.policy.egress_class != spec.security.egress_class {
            return Err("Function invocation egress class drifted from its profile".into());
        }
        if !spec
            .contract
            .input_media_types
            .iter()
            .any(|media_type| media_type == self.input.media_type())
        {
            return Err("Function invocation input media type is not admitted".into());
        }
        if self.input.size_bytes() > spec.policy.max_input_bytes {
            return Err("Function invocation input exceeds its profile bound".into());
        }
        let elapsed = self
            .policy
            .deadline_at
            .signed_duration_since(self.policy.requested_at)
            .num_milliseconds();
        if elapsed <= 0 || elapsed as u64 > spec.policy.timeout_ms {
            return Err("Function invocation deadline exceeds its profile timeout".into());
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        Ok(json_digest(&canonical_json(
            self,
            FUNCTION_INVOCATION_ENVELOPE_MAX_BYTES,
        )?))
    }
}

fn is_millisecond_timestamp(value: DateTime<Utc>) -> bool {
    value.timestamp_subsec_nanos().is_multiple_of(1_000_000)
}
