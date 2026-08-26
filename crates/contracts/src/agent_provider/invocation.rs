use super::{AgentProviderCapabilityV1, AgentProviderProfile};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const HARNESS_INVOCATION_PROFILE_SCHEMA_V1: &str = "a3s.cloud.harness-invocation-profile.v1";
pub const HARNESS_INVOCATION_PROFILE_MAX_BYTES: usize = 256 * 1024;

const MAX_BINDINGS: usize = 128;
const MAX_SAFE_JSON_INTEGER: u64 = 9_007_199_254_740_991;

/// One closed, immutable invocation snapshot resolved by Cloud before dispatch.
///
/// The profile carries only immutable identities, policy digests, and Secret
/// references. Secret material and mutable provider configuration are never
/// part of this contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessInvocationProfileV1 {
    pub schema: String,
    pub agent: HarnessAgentReleaseBindingV1,
    pub provider: HarnessProviderBindingV1,
    pub instructions_digest: String,
    pub environment_policy_digest: String,
    pub security_policy_digest: String,
    pub workspace: HarnessWorkspaceBindingV1,
    pub skills: Vec<HarnessSkillBindingV1>,
    pub mcp_servers: Vec<HarnessMcpBindingV1>,
    pub models: Vec<HarnessModelBindingV1>,
    pub secrets: Vec<HarnessSecretReferenceV1>,
    pub tools: Vec<HarnessToolBindingV1>,
    pub required_capabilities: Vec<AgentProviderCapabilityV1>,
}

impl HarnessInvocationProfileV1 {
    pub const SCHEMA: &'static str = HARNESS_INVOCATION_PROFILE_SCHEMA_V1;

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Harness invocation profile schema {:?}",
                self.schema
            ));
        }
        self.agent.validate()?;
        self.provider.validate()?;
        validate_lower_sha256("Harness instructions digest", &self.instructions_digest)?;
        validate_lower_sha256(
            "Harness environment policy digest",
            &self.environment_policy_digest,
        )?;
        validate_lower_sha256(
            "Harness security policy digest",
            &self.security_policy_digest,
        )?;
        self.workspace.validate()?;
        validate_skills(&self.skills)?;
        validate_mcp_servers(&self.mcp_servers)?;
        validate_models(&self.models)?;
        validate_secrets(&self.secrets)?;
        validate_tools(&self.tools)?;
        validate_capabilities(&self.required_capabilities)?;
        if !self.tools.is_empty()
            && !self
                .required_capabilities
                .contains(&AgentProviderCapabilityV1::ToolCalls)
        {
            return Err("Harness Tool bindings require the tool_calls capability".into());
        }
        canonical_json(self)?;
        Ok(())
    }

    pub fn validate_for(&self, provider: &AgentProviderProfile) -> Result<(), String> {
        self.validate()?;
        provider.validate()?;
        if self.provider.kind != provider.kind()
            || self.provider.revision != provider.revision()
            || self.provider.profile_digest != provider.digest()
            || self.provider.capability_digest != provider.capability_digest()
            || !self
                .required_capabilities
                .iter()
                .all(|capability| provider.supports(*capability))
        {
            return Err(
                "Harness invocation profile does not match its immutable provider profile".into(),
            );
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        Ok(format!(
            "sha256:{:x}",
            Sha256::digest(canonical_json(self)?)
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessAgentReleaseBindingV1 {
    pub organization_id: Uuid,
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub build_run_id: Uuid,
    pub artifact_digest: String,
}

impl HarnessAgentReleaseBindingV1 {
    fn validate(&self) -> Result<(), String> {
        for (label, value) in [
            ("organization ID", self.organization_id),
            ("Agent Asset ID", self.asset_id),
            ("Agent Asset release ID", self.asset_release_id),
            ("Agent BuildRun ID", self.build_run_id),
        ] {
            validate_uuid(label, value)?;
        }
        validate_lower_sha256("Harness Agent artifact digest", &self.artifact_digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessProviderBindingV1 {
    pub kind: String,
    pub revision: String,
    pub profile_digest: String,
    pub capability_digest: String,
}

impl HarnessProviderBindingV1 {
    fn validate(&self) -> Result<(), String> {
        validate_dotted_identifier("Harness provider kind", &self.kind, 64)?;
        validate_single_line("Harness provider revision", &self.revision, 128)?;
        validate_lower_sha256("Harness provider profile digest", &self.profile_digest)?;
        validate_lower_sha256(
            "Harness provider capability digest",
            &self.capability_digest,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessWorkspaceBindingV1 {
    pub workload_id: Uuid,
    pub workload_revision_id: Uuid,
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
    pub runtime_spec_digest: String,
    pub working_directory: Option<String>,
}

impl HarnessWorkspaceBindingV1 {
    fn validate(&self) -> Result<(), String> {
        validate_uuid("Harness workspace Workload ID", self.workload_id)?;
        validate_uuid(
            "Harness workspace Workload revision ID",
            self.workload_revision_id,
        )?;
        validate_single_line("Harness Runtime unit ID", &self.runtime_unit_id, 512)?;
        if self.runtime_generation == 0 || self.runtime_generation > MAX_SAFE_JSON_INTEGER {
            return Err("Harness Runtime generation must be a positive JSON-safe integer".into());
        }
        validate_lower_sha256("Harness Runtime spec digest", &self.runtime_spec_digest)?;
        if let Some(value) = &self.working_directory {
            validate_single_line("Harness working directory", value, 4096)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessSkillBindingV1 {
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub artifact_digest: String,
}

impl HarnessSkillBindingV1 {
    fn validate(&self) -> Result<(), String> {
        validate_uuid("Harness Skill Asset ID", self.asset_id)?;
        validate_uuid("Harness Skill release ID", self.asset_release_id)?;
        validate_lower_sha256("Harness Skill artifact digest", &self.artifact_digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessMcpBindingV1 {
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub profile_digest: String,
}

impl HarnessMcpBindingV1 {
    fn validate(&self) -> Result<(), String> {
        validate_uuid("Harness MCP Asset ID", self.asset_id)?;
        validate_uuid("Harness MCP release ID", self.asset_release_id)?;
        validate_lower_sha256("Harness MCP profile digest", &self.profile_digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessModelBindingV1 {
    pub model_id: Uuid,
    pub model_revision_id: Uuid,
    pub profile_digest: String,
}

impl HarnessModelBindingV1 {
    fn validate(&self) -> Result<(), String> {
        validate_uuid("Harness model ID", self.model_id)?;
        validate_uuid("Harness model revision ID", self.model_revision_id)?;
        validate_lower_sha256("Harness model profile digest", &self.profile_digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessSecretReferenceV1 {
    pub name: String,
    pub secret_id: Uuid,
    pub version: u64,
    pub target: HarnessSecretTargetV1,
}

impl HarnessSecretReferenceV1 {
    fn validate(&self) -> Result<(), String> {
        validate_portable_name("Harness Secret reference name", &self.name, 63)?;
        validate_uuid("Harness Secret ID", self.secret_id)?;
        if self.version == 0 || self.version > MAX_SAFE_JSON_INTEGER {
            return Err("Harness Secret version must be a positive JSON-safe integer".into());
        }
        self.target.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HarnessSecretTargetV1 {
    Environment { variable: String },
    File { path: String, mode: u32 },
    RegistryCredential,
}

impl HarnessSecretTargetV1 {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Environment { variable } => validate_environment_key(variable),
            Self::File { path, mode } => {
                validate_absolute_path(path)?;
                if *mode == 0 || *mode > 0o777 {
                    return Err("Harness Secret file mode is invalid".into());
                }
                Ok(())
            }
            Self::RegistryCredential => Ok(()),
        }
    }

    fn identity(&self) -> String {
        match self {
            Self::Environment { variable } => format!("environment:{variable}"),
            Self::File { path, .. } => format!("file:{path}"),
            Self::RegistryCredential => "registry_credential".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessToolBindingV1 {
    pub name: String,
    pub revision: String,
    pub contract_digest: String,
    pub approval_required: bool,
}

impl HarnessToolBindingV1 {
    pub fn validate(&self) -> Result<(), String> {
        validate_dotted_identifier("Harness Tool name", &self.name, 128)?;
        validate_single_line("Harness Tool revision", &self.revision, 128)?;
        validate_lower_sha256("Harness Tool contract digest", &self.contract_digest)
    }
}

fn validate_skills(values: &[HarnessSkillBindingV1]) -> Result<(), String> {
    validate_count("Skill", values.len())?;
    for value in values {
        value.validate()?;
    }
    if !values.windows(2).all(|pair| {
        (pair[0].asset_id, pair[0].asset_release_id) < (pair[1].asset_id, pair[1].asset_release_id)
    }) {
        return Err("Harness Skill bindings must be sorted and unique".into());
    }
    Ok(())
}

fn validate_mcp_servers(values: &[HarnessMcpBindingV1]) -> Result<(), String> {
    validate_count("MCP", values.len())?;
    for value in values {
        value.validate()?;
    }
    if !values.windows(2).all(|pair| {
        (pair[0].asset_id, pair[0].asset_release_id) < (pair[1].asset_id, pair[1].asset_release_id)
    }) {
        return Err("Harness MCP bindings must be sorted and unique".into());
    }
    Ok(())
}

fn validate_models(values: &[HarnessModelBindingV1]) -> Result<(), String> {
    validate_count("model", values.len())?;
    for value in values {
        value.validate()?;
    }
    if !values.windows(2).all(|pair| {
        (pair[0].model_id, pair[0].model_revision_id)
            < (pair[1].model_id, pair[1].model_revision_id)
    }) {
        return Err("Harness model bindings must be sorted and unique".into());
    }
    Ok(())
}

fn validate_secrets(values: &[HarnessSecretReferenceV1]) -> Result<(), String> {
    validate_count("Secret", values.len())?;
    let mut targets = BTreeSet::new();
    for value in values {
        value.validate()?;
        if !targets.insert(value.target.identity()) {
            return Err("Harness Secret reference targets must be unique".into());
        }
    }
    if !values.windows(2).all(|pair| pair[0].name < pair[1].name) {
        return Err("Harness Secret references must be sorted and unique".into());
    }
    Ok(())
}

fn validate_tools(values: &[HarnessToolBindingV1]) -> Result<(), String> {
    validate_count("Tool", values.len())?;
    for value in values {
        value.validate()?;
    }
    if !values
        .windows(2)
        .all(|pair| (&pair[0].name, &pair[0].revision) < (&pair[1].name, &pair[1].revision))
    {
        return Err("Harness Tool bindings must be sorted and unique".into());
    }
    Ok(())
}

fn validate_capabilities(values: &[AgentProviderCapabilityV1]) -> Result<(), String> {
    if values.is_empty()
        || values.len() > 32
        || !values
            .windows(2)
            .all(|pair| pair[0].as_str() < pair[1].as_str())
    {
        return Err("Harness capability expectations must be sorted, unique, and bounded".into());
    }
    for required in [
        AgentProviderCapabilityV1::Cancellation,
        AgentProviderCapabilityV1::Cleanup,
        AgentProviderCapabilityV1::EventPages,
    ] {
        if !values.contains(&required) {
            return Err(format!(
                "Harness capability expectations omit {:?}",
                required.as_str()
            ));
        }
    }
    Ok(())
}

fn validate_count(label: &str, count: usize) -> Result<(), String> {
    if count > MAX_BINDINGS {
        Err(format!(
            "Harness {label} binding count exceeds {MAX_BINDINGS}"
        ))
    } else {
        Ok(())
    }
}

fn validate_uuid(label: &str, value: Uuid) -> Result<(), String> {
    if value.is_nil() {
        Err(format!("{label} must not be nil"))
    } else {
        Ok(())
    }
}

fn validate_lower_sha256(label: &str, value: &str) -> Result<(), String> {
    let valid = value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if valid {
        Ok(())
    } else {
        Err(format!(
            "{label} must use canonical lowercase SHA-256 syntax"
        ))
    }
}

fn validate_single_line(label: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > max
        || value.contains('\0')
        || value.contains(['\r', '\n'])
    {
        Err(format!(
            "{label} must be a bounded nonempty single-line value"
        ))
    } else {
        Ok(())
    }
}

fn validate_portable_name(label: &str, value: &str, max: usize) -> Result<(), String> {
    if value.is_empty()
        || value.len() > max
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        Err(format!("{label} is invalid"))
    } else {
        Ok(())
    }
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
        Err(format!("{label} must use portable dotted lowercase syntax"))
    } else {
        Ok(())
    }
}

fn validate_environment_key(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 255
        || !value.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_uppercase() || index > 0 && byte.is_ascii_digit()
        })
    {
        Err("Harness Secret environment target is invalid".into())
    } else {
        Ok(())
    }
}

fn validate_absolute_path(value: &str) -> Result<(), String> {
    if !value.starts_with('/')
        || value.len() > 4096
        || value.contains(['\0', '\r', '\n'])
        || value.contains("//")
        || !value
            .split('/')
            .skip(1)
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
    {
        Err("Harness Secret file target is invalid".into())
    } else {
        Ok(())
    }
}

fn canonical_json(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let value = serde_json::to_value(value)
        .map_err(|error| format!("could not project Harness invocation profile: {error}"))?;
    let encoded = serde_json::to_vec(&sort_json(value))
        .map_err(|error| format!("could not encode Harness invocation profile: {error}"))?;
    if encoded.len() > HARNESS_INVOCATION_PROFILE_MAX_BYTES {
        return Err(format!(
            "Harness invocation profile exceeds {} bytes",
            HARNESS_INVOCATION_PROFILE_MAX_BYTES
        ));
    }
    Ok(encoded)
}

fn sort_json(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(sort_json).collect()),
        Value::Object(values) => {
            let sorted = values
                .into_iter()
                .map(|(key, value)| (key, sort_json(value)))
                .collect::<BTreeMap<_, _>>();
            let mut object = Map::new();
            for (key, value) in sorted {
                object.insert(key, value);
            }
            Value::Object(object)
        }
        value => value,
    }
}
