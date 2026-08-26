use a3s_acl::{canonical_bytes, canonical_digest, parse_acl, Block, Document, Value};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const AGENT_PROVIDER_PROFILE_SCHEMA_V1: &str = "a3s.cloud.agent-provider-profile.v1";
pub const AGENT_PROVIDER_PROTOCOL_V1: &str = "a3s.cloud.agent-provider.v1";
pub const NATIVE_CODE_AGENT_PROVIDER_KIND: &str = "a3s.code";
pub const REFERENCE_ECHO_AGENT_PROVIDER_KIND: &str = "reference.echo";
pub const REFERENCE_ECHO_AGENT_PROVIDER_PROTOCOL_V1: &str = "a3s.cloud.reference-echo-agent.v1";

const AGENT_PROVIDER_BLOCK: &str = "agent_provider";
const AGENT_PROVIDER_PROFILE_MAX_ACL_BYTES: usize = 16 * 1024;
const PROFILE_ATTRIBUTES: [&str; 5] = [
    "capabilities",
    "native_protocol",
    "protocol",
    "revision",
    "schema",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProviderCapabilityV1 {
    Cancellation,
    ChangeSet,
    Checkpoints,
    Cleanup,
    EventPages,
    PauseResume,
    Recovery,
    StreamingOutput,
    ToolCalls,
}

impl AgentProviderCapabilityV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cancellation => "cancellation",
            Self::ChangeSet => "change_set",
            Self::Checkpoints => "checkpoints",
            Self::Cleanup => "cleanup",
            Self::EventPages => "event_pages",
            Self::PauseResume => "pause_resume",
            Self::Recovery => "recovery",
            Self::StreamingOutput => "streaming_output",
            Self::ToolCalls => "tool_calls",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "cancellation" => Ok(Self::Cancellation),
            "change_set" => Ok(Self::ChangeSet),
            "checkpoints" => Ok(Self::Checkpoints),
            "cleanup" => Ok(Self::Cleanup),
            "event_pages" => Ok(Self::EventPages),
            "pause_resume" => Ok(Self::PauseResume),
            "recovery" => Ok(Self::Recovery),
            "streaming_output" => Ok(Self::StreamingOutput),
            "tool_calls" => Ok(Self::ToolCalls),
            _ => Err(format!("unknown Agent provider capability {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProviderCapabilityRequirementsV1 {
    pub schema: String,
    pub required: Vec<AgentProviderCapabilityV1>,
}

impl AgentProviderCapabilityRequirementsV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.agent-provider-capability-requirements.v1";

    pub fn new(required: Vec<AgentProviderCapabilityV1>) -> Result<Self, String> {
        let requirements = Self {
            schema: Self::SCHEMA.into(),
            required,
        };
        requirements.validate()?;
        Ok(requirements)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Agent provider capability-requirements schema {:?}",
                self.schema
            ));
        }
        validate_capability_order(&self.required, true)
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        digest_json(self, "Agent provider capability requirements")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProviderCapabilityNegotiationV1 {
    pub schema: String,
    pub provider_profile_digest: String,
    pub provider_capability_digest: String,
    pub requirements_digest: String,
    pub negotiated: Vec<AgentProviderCapabilityV1>,
}

impl AgentProviderCapabilityNegotiationV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.agent-provider-capability-negotiation.v1";

    pub fn validate_for(
        &self,
        profile: &AgentProviderProfile,
        requirements: &AgentProviderCapabilityRequirementsV1,
    ) -> Result<(), String> {
        profile.validate()?;
        requirements.validate()?;
        validate_capability_order(&self.negotiated, true)?;
        if self.schema != Self::SCHEMA
            || self.provider_profile_digest != profile.digest
            || self.provider_capability_digest != profile.capability_digest
            || self.requirements_digest != requirements.digest()?
            || self.negotiated != requirements.required
            || !self
                .negotiated
                .iter()
                .all(|capability| profile.capabilities.contains(capability))
        {
            return Err(
                "Agent provider capability negotiation changed its immutable evidence".into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProviderProfile {
    kind: String,
    revision: String,
    protocol: String,
    native_protocol: String,
    capabilities: Vec<AgentProviderCapabilityV1>,
    canonical_acl: String,
    digest: String,
    capability_digest: String,
}

impl AgentProviderProfile {
    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > AGENT_PROVIDER_PROFILE_MAX_ACL_BYTES {
            return Err("Agent provider profile ACL size is invalid".into());
        }
        if source.contains('\r') && !source.contains("\r\n") {
            return Err("Agent provider profile contains a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized)
            .map_err(|error| format!("Agent provider profile ACL is invalid: {error}"))?;
        let canonical = canonical_bytes(&document)
            .map_err(|error| format!("Agent provider profile is not canonicalizable: {error}"))?;
        if normalized.as_bytes() != canonical {
            return Err("Agent provider profile ACL is not canonical".into());
        }
        let root = exact_root_block(&document)?;
        let kind = root.labels[0].clone();
        validate_dotted_identifier("Agent provider kind", &kind, 64)?;
        let schema = required_string(root, "schema")?;
        if schema != AGENT_PROVIDER_PROFILE_SCHEMA_V1 {
            return Err(format!(
                "unsupported Agent provider profile schema {schema:?}"
            ));
        }
        let protocol = required_string(root, "protocol")?;
        if protocol != AGENT_PROVIDER_PROTOCOL_V1 {
            return Err(format!("unsupported Agent provider protocol {protocol:?}"));
        }
        let revision = required_string(root, "revision")?;
        validate_revision(&revision)?;
        let native_protocol = required_string(root, "native_protocol")?;
        validate_dotted_identifier("Agent provider native protocol", &native_protocol, 128)?;
        let capabilities = required_string_list(root, "capabilities")?
            .iter()
            .map(|value| AgentProviderCapabilityV1::parse(value))
            .collect::<Result<Vec<_>, _>>()?;
        validate_capability_order(&capabilities, false)?;
        for required in [
            AgentProviderCapabilityV1::Cancellation,
            AgentProviderCapabilityV1::Cleanup,
            AgentProviderCapabilityV1::EventPages,
        ] {
            if !capabilities.contains(&required) {
                return Err(format!(
                    "Agent provider profile omits required baseline capability {:?}",
                    required.as_str()
                ));
            }
        }
        let capability_digest = capability_digest(&capabilities)?;
        let digest = canonical_digest(&document)
            .map_err(|error| format!("Agent provider profile digest failed: {error}"))?;
        let canonical_acl = String::from_utf8(canonical)
            .map_err(|_| "Agent provider profile ACL is not UTF-8".to_owned())?;
        let profile = Self {
            kind,
            revision,
            protocol,
            native_protocol,
            capabilities,
            canonical_acl,
            digest,
            capability_digest,
        };
        profile.validate()?;
        Ok(profile)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let profile = Self::parse_acl(source)?;
        if profile.digest != stored_digest {
            return Err("stored Agent provider profile ACL and digest do not match".into());
        }
        Ok(profile)
    }

    pub fn validate(&self) -> Result<(), String> {
        let restored = Self::parse_acl_unchecked(&self.canonical_acl)?;
        if self.kind != restored.kind
            || self.revision != restored.revision
            || self.protocol != restored.protocol
            || self.native_protocol != restored.native_protocol
            || self.capabilities != restored.capabilities
            || self.digest != restored.digest
            || self.capability_digest != restored.capability_digest
        {
            return Err("Agent provider profile changed its canonical identity".into());
        }
        Ok(())
    }

    pub fn negotiate(
        &self,
        requirements: &AgentProviderCapabilityRequirementsV1,
    ) -> Result<AgentProviderCapabilityNegotiationV1, String> {
        self.validate()?;
        requirements.validate()?;
        let unsupported = requirements
            .required
            .iter()
            .filter(|capability| !self.capabilities.contains(capability))
            .map(|capability| capability.as_str())
            .collect::<Vec<_>>();
        if !unsupported.is_empty() {
            return Err(format!(
                "Agent provider {:?} does not support required capabilities: {}",
                self.kind,
                unsupported.join(", ")
            ));
        }
        let negotiation = AgentProviderCapabilityNegotiationV1 {
            schema: AgentProviderCapabilityNegotiationV1::SCHEMA.into(),
            provider_profile_digest: self.digest.clone(),
            provider_capability_digest: self.capability_digest.clone(),
            requirements_digest: requirements.digest()?,
            negotiated: requirements.required.clone(),
        };
        negotiation.validate_for(self, requirements)?;
        Ok(negotiation)
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn protocol(&self) -> &str {
        &self.protocol
    }

    pub fn native_protocol(&self) -> &str {
        &self.native_protocol
    }

    pub fn capabilities(&self) -> &[AgentProviderCapabilityV1] {
        &self.capabilities
    }

    pub fn supports(&self, capability: AgentProviderCapabilityV1) -> bool {
        self.capabilities.contains(&capability)
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn capability_digest(&self) -> &str {
        &self.capability_digest
    }

    fn parse_acl_unchecked(source: &str) -> Result<Self, String> {
        Self::parse_acl_inner(source)
    }

    fn parse_acl_inner(source: &str) -> Result<Self, String> {
        // `validate` needs an independently parsed value without recursively
        // validating that value again.
        if source.is_empty() || source.len() > AGENT_PROVIDER_PROFILE_MAX_ACL_BYTES {
            return Err("Agent provider profile ACL size is invalid".into());
        }
        let document = parse_acl(source)
            .map_err(|error| format!("Agent provider profile ACL is invalid: {error}"))?;
        let canonical = canonical_bytes(&document)
            .map_err(|error| format!("Agent provider profile is not canonicalizable: {error}"))?;
        if source.as_bytes() != canonical {
            return Err("Agent provider profile ACL is not canonical".into());
        }
        let root = exact_root_block(&document)?;
        let kind = root.labels[0].clone();
        let revision = required_string(root, "revision")?;
        let protocol = required_string(root, "protocol")?;
        let native_protocol = required_string(root, "native_protocol")?;
        let capabilities = required_string_list(root, "capabilities")?
            .iter()
            .map(|value| AgentProviderCapabilityV1::parse(value))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            kind,
            revision,
            protocol,
            native_protocol,
            capability_digest: capability_digest(&capabilities)?,
            capabilities,
            canonical_acl: String::from_utf8(canonical)
                .map_err(|_| "Agent provider profile ACL is not UTF-8".to_owned())?,
            digest: canonical_digest(&document)
                .map_err(|error| format!("Agent provider profile digest failed: {error}"))?,
        })
    }
}

fn exact_root_block(document: &Document) -> Result<&Block, String> {
    if document.blocks.len() != 1 {
        return Err("Agent provider profile requires exactly one root block".into());
    }
    let root = &document.blocks[0];
    if root.name != AGENT_PROVIDER_BLOCK
        || root.labels.len() != 1
        || !root.blocks.is_empty()
        || root.attributes.len() != PROFILE_ATTRIBUTES.len()
        || PROFILE_ATTRIBUTES
            .iter()
            .any(|name| !root.attributes.contains_key(*name))
    {
        return Err("Agent provider profile root shape is invalid".into());
    }
    Ok(root)
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("Agent provider profile field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("Agent provider profile field {name:?} must be a string"))
}

fn required_string_list(block: &Block, name: &str) -> Result<Vec<String>, String> {
    let Value::List(values) = required_value(block, name)? else {
        return Err(format!(
            "Agent provider profile field {name:?} must be a string list"
        ));
    };
    values
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                format!("Agent provider profile field {name:?} must be a string list")
            })
        })
        .collect()
}

fn validate_capability_order(
    capabilities: &[AgentProviderCapabilityV1],
    allow_empty: bool,
) -> Result<(), String> {
    if (!allow_empty && capabilities.is_empty())
        || capabilities.len() > 32
        || !capabilities
            .windows(2)
            .all(|pair| pair[0].as_str() < pair[1].as_str())
    {
        return Err("Agent provider capabilities must be a sorted unique bounded list".into());
    }
    Ok(())
}

fn capability_digest(capabilities: &[AgentProviderCapabilityV1]) -> Result<String, String> {
    validate_capability_order(capabilities, false)?;
    let values = capabilities
        .iter()
        .map(|capability| capability.as_str())
        .collect::<Vec<_>>();
    digest_json(&values, "Agent provider capabilities")
}

fn digest_json(value: &impl Serialize, label: &str) -> Result<String, String> {
    let encoded =
        serde_json::to_vec(value).map_err(|error| format!("could not encode {label}: {error}"))?;
    Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
}

fn validate_dotted_identifier(label: &str, value: &str, max: usize) -> Result<(), String> {
    if value.is_empty()
        || value.len() > max
        || value.split('.').any(|segment| {
            segment.is_empty()
                || segment.starts_with('-')
                || segment.ends_with('-')
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return Err(format!("{label} must use portable dotted lowercase syntax"));
    }
    Ok(())
}

fn validate_revision(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || value
            .as_bytes()
            .first()
            .is_some_and(|byte| matches!(*byte, b'.' | b'-' | b'+'))
        || value
            .as_bytes()
            .last()
            .is_some_and(|byte| matches!(*byte, b'.' | b'-' | b'+'))
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
    {
        return Err("Agent provider revision is invalid".into());
    }
    Ok(())
}
