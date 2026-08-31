use super::validation::{
    validate_digest, validate_dotted_identifier, validate_media_type, validate_portable_name,
    validate_uuid, MAX_SAFE_INTEGER,
};
use crate::RuntimeIsolationLevel;
use a3s_acl::builder::{integer, list, string, BlockBuilder};
use a3s_acl::{canonical_bytes, canonical_digest, parse_acl, Block, Document, Value};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const FUNCTION_PROFILE_SCHEMA_V1: &str = "cloud.function.profile.v1";
pub const FUNCTION_PROFILE_MAX_ACL_BYTES: usize = 128 * 1024;
pub const FUNCTION_MAX_CONCURRENCY: u64 = 4_096;
pub const FUNCTION_HOSTED_TASK_MAX_TIMEOUT_MS: u64 = 24 * 60 * 60 * 1_000;
pub const FUNCTION_HOSTED_TASK_MAX_INPUT_BYTES: u64 = 64 * 1024 * 1024;
pub const FUNCTION_HOSTED_TASK_MAX_OUTPUT_BYTES: u64 = 256 * 1024 * 1024;
pub const FUNCTION_HOSTED_SERVICE_MAX_TIMEOUT_MS: u64 = 120 * 1_000;
pub const FUNCTION_HOSTED_SERVICE_MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;
pub const FUNCTION_HOSTED_SERVICE_MAX_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;
pub const FUNCTION_EXTERNAL_MAX_TIMEOUT_MS: u64 = 120 * 1_000;
pub const FUNCTION_EXTERNAL_MAX_INPUT_BYTES: u64 = 1024 * 1024;
pub const FUNCTION_EXTERNAL_MAX_OUTPUT_BYTES: u64 = 1024 * 1024;

const PROFILE_BLOCK: &str = "function_profile";
const PROFILE_ATTRIBUTES: [&str; 5] = [
    "asset_id",
    "asset_release_id",
    "mode",
    "organization_id",
    "schema",
];
const CONTRACT_ATTRIBUTES: [&str; 4] = [
    "input_media_types",
    "input_schema_digest",
    "output_media_types",
    "output_schema_digest",
];
const POLICY_REQUIRED_ATTRIBUTES: [&str; 4] = [
    "max_concurrency",
    "max_input_bytes",
    "max_output_bytes",
    "timeout_ms",
];
const POLICY_OPTIONAL_ATTRIBUTES: [&str; 1] = ["isolation"];
const SECURITY_ATTRIBUTES: [&str; 2] = ["egress_class", "grant_requirements"];
const HOSTED_TASK_ATTRIBUTES: [&str; 5] = [
    "artifact_digest",
    "artifact_media_type",
    "execution_template_id",
    "execution_template_revision_id",
    "projection_digest",
];
const HOSTED_SERVICE_ATTRIBUTES: [&str; 5] = [
    "artifact_digest",
    "artifact_media_type",
    "projection_digest",
    "workload_id",
    "workload_revision_id",
];
const EXTERNAL_ATTRIBUTES: [&str; 3] = [
    "connector_definition_digest",
    "connector_profile_id",
    "connector_revision_id",
];
const TRAFFIC_ATTRIBUTES: [&str; 4] = ["path", "protocol", "runtime_port", "visibility"];
const SECRET_ATTRIBUTES: [&str; 2] = ["secret_id", "version"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionModeV1 {
    HostedTask,
    HostedService,
    External,
}

impl FunctionModeV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HostedTask => "hosted_task",
            Self::HostedService => "hosted_service",
            Self::External => "external",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "hosted_task" => Ok(Self::HostedTask),
            "hosted_service" => Ok(Self::HostedService),
            "external" => Ok(Self::External),
            _ => Err(format!("unsupported Function mode {value:?}")),
        }
    }

    pub const fn owner(self) -> FunctionOwnerV1 {
        match self {
            Self::HostedTask => FunctionOwnerV1::Executions,
            Self::HostedService => FunctionOwnerV1::Workloads,
            Self::External => FunctionOwnerV1::Connectors,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionOwnerV1 {
    Executions,
    Workloads,
    Connectors,
}

impl FunctionOwnerV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Executions => "executions",
            Self::Workloads => "workloads",
            Self::Connectors => "connectors",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionEgressClassV1 {
    Denied,
    Restricted,
    Public,
}

impl FunctionEgressClassV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Denied => "denied",
            Self::Restricted => "restricted",
            Self::Public => "public",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "denied" => Ok(Self::Denied),
            "restricted" => Ok(Self::Restricted),
            "public" => Ok(Self::Public),
            _ => Err(format!("unsupported Function egress class {value:?}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionTrafficProtocolV1 {
    Http,
    McpSessionless,
}

impl FunctionTrafficProtocolV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::McpSessionless => "mcp_sessionless",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "http" => Ok(Self::Http),
            "mcp_sessionless" => Ok(Self::McpSessionless),
            _ => Err(format!("unsupported Function traffic protocol {value:?}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionTrafficVisibilityV1 {
    Internal,
    Public,
}

impl FunctionTrafficVisibilityV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::Public => "public",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "internal" => Ok(Self::Internal),
            "public" => Ok(Self::Public),
            _ => Err(format!("unsupported Function traffic visibility {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionIoContractV1 {
    pub input_schema_digest: String,
    pub output_schema_digest: String,
    pub input_media_types: Vec<String>,
    pub output_media_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionPolicyV1 {
    pub timeout_ms: u64,
    pub max_input_bytes: u64,
    pub max_output_bytes: u64,
    pub max_concurrency: u64,
    pub isolation: Option<RuntimeIsolationLevel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionSecretReferenceV1 {
    pub name: String,
    pub secret_id: Uuid,
    pub version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionSecurityV1 {
    pub egress_class: FunctionEgressClassV1,
    pub grant_requirements: Vec<String>,
    pub secrets: Vec<FunctionSecretReferenceV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedTaskFunctionTargetV1 {
    pub artifact_digest: String,
    pub artifact_media_type: String,
    pub execution_template_id: Uuid,
    pub execution_template_revision_id: Uuid,
    pub projection_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedServiceFunctionTargetV1 {
    pub artifact_digest: String,
    pub artifact_media_type: String,
    pub workload_id: Uuid,
    pub workload_revision_id: Uuid,
    pub projection_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalFunctionTargetV1 {
    pub connector_profile_id: Uuid,
    pub connector_revision_id: Uuid,
    pub connector_definition_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "binding",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FunctionTargetV1 {
    HostedTask(HostedTaskFunctionTargetV1),
    HostedService(HostedServiceFunctionTargetV1),
    External(ExternalFunctionTargetV1),
}

impl FunctionTargetV1 {
    pub const fn mode(&self) -> FunctionModeV1 {
        match self {
            Self::HostedTask(_) => FunctionModeV1::HostedTask,
            Self::HostedService(_) => FunctionModeV1::HostedService,
            Self::External(_) => FunctionModeV1::External,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionTrafficV1 {
    pub protocol: FunctionTrafficProtocolV1,
    pub visibility: FunctionTrafficVisibilityV1,
    pub path: String,
    pub runtime_port: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionProfileSpecV1 {
    pub schema: String,
    pub organization_id: Uuid,
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub contract: FunctionIoContractV1,
    pub policy: FunctionPolicyV1,
    pub security: FunctionSecurityV1,
    pub target: FunctionTargetV1,
    pub traffic: Option<FunctionTrafficV1>,
}

/// Canonical product intent for one immutable Function Asset release.
/// Exact lifecycle state stays with the owner selected by [`FunctionModeV1`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionProfileV1 {
    spec: FunctionProfileSpecV1,
    canonical_acl: String,
    digest: String,
}

impl FunctionProfileV1 {
    pub const SCHEMA: &'static str = FUNCTION_PROFILE_SCHEMA_V1;

    pub fn from_spec(mut spec: FunctionProfileSpecV1) -> Result<Self, String> {
        normalize_spec(&mut spec);
        validate_spec(&spec)?;
        let document = profile_document(&spec)?;
        let canonical_acl = String::from_utf8(
            canonical_bytes(&document)
                .map_err(|error| format!("Function profile is not canonicalizable: {error}"))?,
        )
        .map_err(|_| "Function profile canonical ACL is not UTF-8".to_owned())?;
        if canonical_acl.len() > FUNCTION_PROFILE_MAX_ACL_BYTES {
            return Err("Function profile ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated Function profile ACL is invalid: {error}"))?;
        if parse_profile_document(&reparsed)? != spec {
            return Err("generated Function profile ACL changed its semantic value".into());
        }
        let digest = canonical_digest(&reparsed)
            .map_err(|error| format!("Function profile is not canonicalizable: {error}"))?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > FUNCTION_PROFILE_MAX_ACL_BYTES {
            return Err("Function profile ACL size is invalid".into());
        }
        if source.contains('\r') && !source.contains("\r\n") {
            return Err("Function profile ACL contains a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized)
            .map_err(|error| format!("Function profile ACL is invalid: {error}"))?;
        let profile = Self::from_spec(parse_profile_document(&document)?)?;
        if normalized != profile.canonical_acl {
            return Err("Function profile ACL is not canonical".into());
        }
        Ok(profile)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let profile = Self::parse_acl(source)?;
        if profile.canonical_acl != source || profile.digest != stored_digest {
            return Err("stored Function profile ACL and digest do not match".into());
        }
        Ok(profile)
    }

    pub const fn spec(&self) -> &FunctionProfileSpecV1 {
        &self.spec
    }

    pub const fn mode(&self) -> FunctionModeV1 {
        self.spec.target.mode()
    }

    pub const fn owner(&self) -> FunctionOwnerV1 {
        self.mode().owner()
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

fn normalize_spec(spec: &mut FunctionProfileSpecV1) {
    spec.contract.input_media_types.sort_unstable();
    spec.contract.output_media_types.sort_unstable();
    spec.security.grant_requirements.sort_unstable();
    spec.security
        .secrets
        .sort_unstable_by(|left, right| left.name.cmp(&right.name));
}

fn validate_spec(spec: &FunctionProfileSpecV1) -> Result<(), String> {
    if spec.schema != FUNCTION_PROFILE_SCHEMA_V1 {
        return Err(format!(
            "unsupported Function profile schema {:?}",
            spec.schema
        ));
    }
    validate_uuid("Function organization ID", spec.organization_id)?;
    validate_uuid("Function Asset ID", spec.asset_id)?;
    validate_uuid("Function Asset release ID", spec.asset_release_id)?;
    validate_io_contract(&spec.contract)?;
    validate_policy(&spec.policy, spec.target.mode())?;
    validate_security(&spec.security, spec.target.mode())?;
    validate_target(&spec.target)?;
    match (spec.target.mode(), &spec.traffic) {
        (FunctionModeV1::HostedService, Some(traffic)) => {
            validate_traffic(traffic)?;
            if traffic.protocol == FunctionTrafficProtocolV1::McpSessionless
                && (!spec
                    .contract
                    .input_media_types
                    .iter()
                    .any(|value| value == "application/json")
                    || !spec
                        .contract
                        .output_media_types
                        .iter()
                        .any(|value| value == "application/json"))
            {
                return Err(
                    "sessionless MCP Function traffic requires JSON input and output".into(),
                );
            }
        }
        (FunctionModeV1::HostedService, None) => {}
        (_, None) => {}
        (_, Some(_)) => {
            return Err("Function traffic intent is valid only for hosted_service mode".into())
        }
    }
    Ok(())
}

fn validate_io_contract(contract: &FunctionIoContractV1) -> Result<(), String> {
    validate_digest(
        "Function input schema digest",
        &contract.input_schema_digest,
    )?;
    validate_digest(
        "Function output schema digest",
        &contract.output_schema_digest,
    )?;
    for (label, values) in [
        ("input", &contract.input_media_types),
        ("output", &contract.output_media_types),
    ] {
        if values.is_empty()
            || values.len() > 16
            || !values.windows(2).all(|pair| pair[0] < pair[1])
        {
            return Err(format!(
                "Function {label} media types must be sorted, unique, and bounded"
            ));
        }
        for value in values {
            validate_media_type(&format!("Function {label} media type"), value)?;
        }
    }
    Ok(())
}

fn validate_policy(policy: &FunctionPolicyV1, mode: FunctionModeV1) -> Result<(), String> {
    let (maximum_timeout, maximum_input, maximum_output, isolation_required) = match mode {
        FunctionModeV1::HostedTask => (
            FUNCTION_HOSTED_TASK_MAX_TIMEOUT_MS,
            FUNCTION_HOSTED_TASK_MAX_INPUT_BYTES,
            FUNCTION_HOSTED_TASK_MAX_OUTPUT_BYTES,
            true,
        ),
        FunctionModeV1::HostedService => (
            FUNCTION_HOSTED_SERVICE_MAX_TIMEOUT_MS,
            FUNCTION_HOSTED_SERVICE_MAX_INPUT_BYTES,
            FUNCTION_HOSTED_SERVICE_MAX_OUTPUT_BYTES,
            true,
        ),
        FunctionModeV1::External => (
            FUNCTION_EXTERNAL_MAX_TIMEOUT_MS,
            FUNCTION_EXTERNAL_MAX_INPUT_BYTES,
            FUNCTION_EXTERNAL_MAX_OUTPUT_BYTES,
            false,
        ),
    };
    if policy.timeout_ms == 0
        || policy.timeout_ms > maximum_timeout
        || policy.max_input_bytes == 0
        || policy.max_input_bytes > maximum_input
        || policy.max_output_bytes == 0
        || policy.max_output_bytes > maximum_output
        || policy.max_concurrency == 0
        || policy.max_concurrency > FUNCTION_MAX_CONCURRENCY
        || policy.timeout_ms > MAX_SAFE_INTEGER
        || policy.max_input_bytes > MAX_SAFE_INTEGER
        || policy.max_output_bytes > MAX_SAFE_INTEGER
    {
        return Err(format!(
            "Function {mode:?} policy exceeds its closed bounds"
        ));
    }
    match (isolation_required, policy.isolation) {
        (true, Some(_)) | (false, None) => Ok(()),
        (true, None) => Err("hosted Function policy requires an exact Runtime isolation".into()),
        (false, Some(_)) => {
            Err("external Function policy cannot declare local Runtime isolation".into())
        }
    }
}

fn validate_security(security: &FunctionSecurityV1, mode: FunctionModeV1) -> Result<(), String> {
    if security.grant_requirements.is_empty()
        || security.grant_requirements.len() > 32
        || !security
            .grant_requirements
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    {
        return Err("Function grant requirements must be sorted, unique, and bounded".into());
    }
    for requirement in &security.grant_requirements {
        validate_dotted_identifier("Function grant requirement", requirement, 128)?;
    }
    if !security
        .grant_requirements
        .iter()
        .any(|requirement| requirement == "function.invoke")
    {
        return Err("Function profile must require function.invoke authorization".into());
    }
    if security.secrets.len() > 64
        || !security
            .secrets
            .windows(2)
            .all(|pair| pair[0].name < pair[1].name)
    {
        return Err("Function Secret references must be sorted, unique, and bounded".into());
    }
    for secret in &security.secrets {
        validate_portable_name("Function Secret reference name", &secret.name, 128)?;
        validate_uuid("Function Secret ID", secret.secret_id)?;
        if secret.version == 0 || secret.version > MAX_SAFE_INTEGER {
            return Err("Function Secret version must be a positive ACL-safe integer".into());
        }
    }
    if mode == FunctionModeV1::External {
        if security.egress_class == FunctionEgressClassV1::Denied {
            return Err("external Function mode requires admitted Connector egress".into());
        }
        if !security.secrets.is_empty() {
            return Err(
                "external Function credentials belong to the exact Connector revision".into(),
            );
        }
    }
    Ok(())
}

fn validate_target(target: &FunctionTargetV1) -> Result<(), String> {
    match target {
        FunctionTargetV1::HostedTask(target) => {
            validate_digest("Function artifact digest", &target.artifact_digest)?;
            validate_media_type("Function artifact media type", &target.artifact_media_type)?;
            validate_uuid(
                "Function ExecutionTemplate ID",
                target.execution_template_id,
            )?;
            validate_uuid(
                "Function ExecutionTemplate revision ID",
                target.execution_template_revision_id,
            )?;
            validate_digest("Function Task projection digest", &target.projection_digest)
        }
        FunctionTargetV1::HostedService(target) => {
            validate_digest("Function artifact digest", &target.artifact_digest)?;
            validate_media_type("Function artifact media type", &target.artifact_media_type)?;
            validate_uuid("Function Workload ID", target.workload_id)?;
            validate_uuid("Function Workload revision ID", target.workload_revision_id)?;
            validate_digest(
                "Function Service projection digest",
                &target.projection_digest,
            )
        }
        FunctionTargetV1::External(target) => {
            validate_uuid("Function Connector profile ID", target.connector_profile_id)?;
            validate_uuid(
                "Function Connector revision ID",
                target.connector_revision_id,
            )?;
            validate_digest(
                "Function Connector definition digest",
                &target.connector_definition_digest,
            )
        }
    }
}

fn validate_traffic(traffic: &FunctionTrafficV1) -> Result<(), String> {
    if traffic.path.is_empty()
        || traffic.path.len() > 1_024
        || !traffic.path.starts_with('/')
        || traffic.path.starts_with("//")
        || traffic.path.contains(['?', '#', '%', '*', '{', '}', '`'])
        || traffic
            .path
            .split('/')
            .any(|segment| matches!(segment, "." | ".."))
        || traffic
            .path
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err("Function traffic path must be one safe literal path".into());
    }
    validate_portable_name("Function Runtime port", &traffic.runtime_port, 63)
}

fn profile_document(spec: &FunctionProfileSpecV1) -> Result<Document, String> {
    let contract = BlockBuilder::new("contract")
        .attr(
            "input_schema_digest",
            string(&spec.contract.input_schema_digest),
        )
        .attr(
            "output_schema_digest",
            string(&spec.contract.output_schema_digest),
        )
        .attr(
            "input_media_types",
            list(
                spec.contract
                    .input_media_types
                    .iter()
                    .map(|value| string(value))
                    .collect(),
            ),
        )
        .attr(
            "output_media_types",
            list(
                spec.contract
                    .output_media_types
                    .iter()
                    .map(|value| string(value))
                    .collect(),
            ),
        )
        .build();
    let mut policy = BlockBuilder::new("policy")
        .attr(
            "timeout_ms",
            acl_integer("timeout_ms", spec.policy.timeout_ms)?,
        )
        .attr(
            "max_input_bytes",
            acl_integer("max_input_bytes", spec.policy.max_input_bytes)?,
        )
        .attr(
            "max_output_bytes",
            acl_integer("max_output_bytes", spec.policy.max_output_bytes)?,
        )
        .attr(
            "max_concurrency",
            acl_integer("max_concurrency", spec.policy.max_concurrency)?,
        );
    if let Some(isolation) = spec.policy.isolation {
        policy = policy.attr("isolation", string(isolation_as_str(isolation)));
    }
    let mut security = BlockBuilder::new("security")
        .attr("egress_class", string(spec.security.egress_class.as_str()))
        .attr(
            "grant_requirements",
            list(
                spec.security
                    .grant_requirements
                    .iter()
                    .map(|value| string(value))
                    .collect(),
            ),
        );
    for secret in &spec.security.secrets {
        security = security.nested_block(
            BlockBuilder::new("secret")
                .label(&secret.name)
                .attr("secret_id", string(&secret.secret_id.to_string()))
                .attr("version", acl_integer("Secret version", secret.version)?)
                .build(),
        );
    }
    let mut root = BlockBuilder::new(PROFILE_BLOCK)
        .attr("schema", string(FUNCTION_PROFILE_SCHEMA_V1))
        .attr("organization_id", string(&spec.organization_id.to_string()))
        .attr("asset_id", string(&spec.asset_id.to_string()))
        .attr(
            "asset_release_id",
            string(&spec.asset_release_id.to_string()),
        )
        .attr("mode", string(spec.target.mode().as_str()))
        .nested_block(contract)
        .nested_block(policy.build())
        .nested_block(security.build())
        .nested_block(target_block(&spec.target));
    if let Some(traffic) = &spec.traffic {
        root = root.nested_block(
            BlockBuilder::new("traffic")
                .attr("protocol", string(traffic.protocol.as_str()))
                .attr("visibility", string(traffic.visibility.as_str()))
                .attr("path", string(&traffic.path))
                .attr("runtime_port", string(&traffic.runtime_port))
                .build(),
        );
    }
    Ok(Document {
        blocks: vec![root.build()],
    })
}

fn target_block(target: &FunctionTargetV1) -> Block {
    match target {
        FunctionTargetV1::HostedTask(target) => BlockBuilder::new("hosted_task")
            .attr("artifact_digest", string(&target.artifact_digest))
            .attr("artifact_media_type", string(&target.artifact_media_type))
            .attr(
                "execution_template_id",
                string(&target.execution_template_id.to_string()),
            )
            .attr(
                "execution_template_revision_id",
                string(&target.execution_template_revision_id.to_string()),
            )
            .attr("projection_digest", string(&target.projection_digest))
            .build(),
        FunctionTargetV1::HostedService(target) => BlockBuilder::new("hosted_service")
            .attr("artifact_digest", string(&target.artifact_digest))
            .attr("artifact_media_type", string(&target.artifact_media_type))
            .attr("workload_id", string(&target.workload_id.to_string()))
            .attr(
                "workload_revision_id",
                string(&target.workload_revision_id.to_string()),
            )
            .attr("projection_digest", string(&target.projection_digest))
            .build(),
        FunctionTargetV1::External(target) => BlockBuilder::new("external")
            .attr(
                "connector_profile_id",
                string(&target.connector_profile_id.to_string()),
            )
            .attr(
                "connector_revision_id",
                string(&target.connector_revision_id.to_string()),
            )
            .attr(
                "connector_definition_digest",
                string(&target.connector_definition_digest),
            )
            .build(),
    }
}

fn parse_profile_document(document: &Document) -> Result<FunctionProfileSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err("Function profile must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_attributes(root, PROFILE_BLOCK, &PROFILE_ATTRIBUTES, &[], 0)?;
    let mode = FunctionModeV1::parse(&required_string(root, "mode")?)?;
    let expected_nested = if mode == FunctionModeV1::HostedService {
        4..=5
    } else {
        4..=4
    };
    if !expected_nested.contains(&root.blocks.len())
        || root.blocks.iter().any(|block| {
            !["contract", "policy", "security", mode.as_str(), "traffic"]
                .contains(&block.name.as_str())
        })
    {
        return Err("Function profile contains a missing or foreign authority block".into());
    }
    let contract = exact_nested(root, "contract")?;
    exact_attributes(contract, "contract", &CONTRACT_ATTRIBUTES, &[], 0)?;
    if !contract.blocks.is_empty() {
        return Err("Function contract block cannot contain nested state".into());
    }
    let policy = exact_nested(root, "policy")?;
    exact_attributes(
        policy,
        "policy",
        &POLICY_REQUIRED_ATTRIBUTES,
        &POLICY_OPTIONAL_ATTRIBUTES,
        0,
    )?;
    if !policy.blocks.is_empty() {
        return Err("Function policy block cannot contain nested state".into());
    }
    let security = exact_nested(root, "security")?;
    exact_attributes(security, "security", &SECURITY_ATTRIBUTES, &[], 0)?;
    if security.blocks.iter().any(|block| block.name != "secret") {
        return Err("Function security block contains an unknown child".into());
    }
    let secrets = security
        .blocks
        .iter()
        .map(parse_secret)
        .collect::<Result<Vec<_>, _>>()?;
    let target = parse_target(mode, exact_nested(root, mode.as_str())?)?;
    let traffic = optional_nested(root, "traffic")?
        .map(parse_traffic)
        .transpose()?;
    if mode != FunctionModeV1::HostedService && traffic.is_some() {
        return Err("Function traffic intent is valid only for hosted_service mode".into());
    }
    Ok(FunctionProfileSpecV1 {
        schema: required_string(root, "schema")?,
        organization_id: required_uuid(root, "organization_id")?,
        asset_id: required_uuid(root, "asset_id")?,
        asset_release_id: required_uuid(root, "asset_release_id")?,
        contract: FunctionIoContractV1 {
            input_schema_digest: required_string(contract, "input_schema_digest")?,
            output_schema_digest: required_string(contract, "output_schema_digest")?,
            input_media_types: required_strings(contract, "input_media_types")?,
            output_media_types: required_strings(contract, "output_media_types")?,
        },
        policy: FunctionPolicyV1 {
            timeout_ms: required_positive_u64(policy, "timeout_ms")?,
            max_input_bytes: required_positive_u64(policy, "max_input_bytes")?,
            max_output_bytes: required_positive_u64(policy, "max_output_bytes")?,
            max_concurrency: required_positive_u64(policy, "max_concurrency")?,
            isolation: optional_string(policy, "isolation")?
                .map(|value| parse_isolation(&value))
                .transpose()?,
        },
        security: FunctionSecurityV1 {
            egress_class: FunctionEgressClassV1::parse(&required_string(
                security,
                "egress_class",
            )?)?,
            grant_requirements: required_strings(security, "grant_requirements")?,
            secrets,
        },
        target,
        traffic,
    })
}

fn parse_secret(block: &Block) -> Result<FunctionSecretReferenceV1, String> {
    exact_attributes(block, "secret", &SECRET_ATTRIBUTES, &[], 1)?;
    if !block.blocks.is_empty() {
        return Err("Function Secret reference cannot contain nested state".into());
    }
    Ok(FunctionSecretReferenceV1 {
        name: block.labels[0].clone(),
        secret_id: required_uuid(block, "secret_id")?,
        version: required_positive_u64(block, "version")?,
    })
}

fn parse_target(mode: FunctionModeV1, block: &Block) -> Result<FunctionTargetV1, String> {
    if !block.blocks.is_empty() {
        return Err("Function target cannot contain nested lifecycle state".into());
    }
    match mode {
        FunctionModeV1::HostedTask => {
            exact_attributes(block, "hosted_task", &HOSTED_TASK_ATTRIBUTES, &[], 0)?;
            Ok(FunctionTargetV1::HostedTask(HostedTaskFunctionTargetV1 {
                artifact_digest: required_string(block, "artifact_digest")?,
                artifact_media_type: required_string(block, "artifact_media_type")?,
                execution_template_id: required_uuid(block, "execution_template_id")?,
                execution_template_revision_id: required_uuid(
                    block,
                    "execution_template_revision_id",
                )?,
                projection_digest: required_string(block, "projection_digest")?,
            }))
        }
        FunctionModeV1::HostedService => {
            exact_attributes(block, "hosted_service", &HOSTED_SERVICE_ATTRIBUTES, &[], 0)?;
            Ok(FunctionTargetV1::HostedService(
                HostedServiceFunctionTargetV1 {
                    artifact_digest: required_string(block, "artifact_digest")?,
                    artifact_media_type: required_string(block, "artifact_media_type")?,
                    workload_id: required_uuid(block, "workload_id")?,
                    workload_revision_id: required_uuid(block, "workload_revision_id")?,
                    projection_digest: required_string(block, "projection_digest")?,
                },
            ))
        }
        FunctionModeV1::External => {
            exact_attributes(block, "external", &EXTERNAL_ATTRIBUTES, &[], 0)?;
            Ok(FunctionTargetV1::External(ExternalFunctionTargetV1 {
                connector_profile_id: required_uuid(block, "connector_profile_id")?,
                connector_revision_id: required_uuid(block, "connector_revision_id")?,
                connector_definition_digest: required_string(block, "connector_definition_digest")?,
            }))
        }
    }
}

fn parse_traffic(block: &Block) -> Result<FunctionTrafficV1, String> {
    exact_attributes(block, "traffic", &TRAFFIC_ATTRIBUTES, &[], 0)?;
    if !block.blocks.is_empty() {
        return Err("Function traffic block cannot contain mutable route state".into());
    }
    Ok(FunctionTrafficV1 {
        protocol: FunctionTrafficProtocolV1::parse(&required_string(block, "protocol")?)?,
        visibility: FunctionTrafficVisibilityV1::parse(&required_string(block, "visibility")?)?,
        path: required_string(block, "path")?,
        runtime_port: required_string(block, "runtime_port")?,
    })
}

fn exact_attributes(
    block: &Block,
    name: &str,
    required: &[&str],
    optional: &[&str],
    labels: usize,
) -> Result<(), String> {
    if block.name != name
        || block.labels.len() != labels
        || required
            .iter()
            .any(|attribute| !block.attributes.contains_key(*attribute))
        || block.attributes.keys().any(|attribute| {
            !required.contains(&attribute.as_str()) && !optional.contains(&attribute.as_str())
        })
    {
        return Err(format!(
            "Function profile {name} block contains missing or unknown fields"
        ));
    }
    Ok(())
}

fn exact_nested<'a>(root: &'a Block, name: &str) -> Result<&'a Block, String> {
    let mut matches = root.blocks.iter().filter(|block| block.name == name);
    let value = matches
        .next()
        .ok_or_else(|| format!("Function profile {name} block is required"))?;
    if matches.next().is_some() {
        return Err(format!("Function profile {name} block must be unique"));
    }
    Ok(value)
}

fn optional_nested<'a>(root: &'a Block, name: &str) -> Result<Option<&'a Block>, String> {
    let mut matches = root.blocks.iter().filter(|block| block.name == name);
    let value = matches.next();
    if matches.next().is_some() {
        return Err(format!("Function profile {name} block must be unique"));
    }
    Ok(value)
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("Function profile field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("Function profile field {name:?} must be a string"))
}

fn optional_string(block: &Block, name: &str) -> Result<Option<String>, String> {
    block
        .attributes
        .get(name)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("Function profile field {name:?} must be a string"))
        })
        .transpose()
}

fn required_uuid(block: &Block, name: &str) -> Result<Uuid, String> {
    Uuid::parse_str(&required_string(block, name)?)
        .map_err(|_| format!("Function profile field {name:?} must be a UUID"))
}

fn required_strings(block: &Block, name: &str) -> Result<Vec<String>, String> {
    let Value::List(values) = required_value(block, name)? else {
        return Err(format!(
            "Function profile field {name:?} must be a string list"
        ));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("Function profile field {name:?} must be a string list"))
        })
        .collect()
}

fn required_positive_u64(block: &Block, name: &str) -> Result<u64, String> {
    let value = required_value(block, name)?
        .as_number()
        .ok_or_else(|| format!("Function profile field {name:?} must be an integer"))?;
    if !value.is_finite() || value.fract() != 0.0 || value <= 0.0 || value > MAX_SAFE_INTEGER as f64
    {
        return Err(format!(
            "Function profile field {name:?} must be a positive exactly representable integer"
        ));
    }
    Ok(value as u64)
}

fn acl_integer(name: &str, value: u64) -> Result<Value, String> {
    if value == 0 || value > MAX_SAFE_INTEGER {
        return Err(format!(
            "Function profile {name} is not representable by ACL"
        ));
    }
    Ok(integer(value as i64))
}

fn isolation_as_str(value: RuntimeIsolationLevel) -> &'static str {
    match value {
        RuntimeIsolationLevel::Process => "process",
        RuntimeIsolationLevel::Container => "container",
        RuntimeIsolationLevel::Sandbox => "sandbox",
        RuntimeIsolationLevel::Confidential => "confidential",
    }
}

fn parse_isolation(value: &str) -> Result<RuntimeIsolationLevel, String> {
    match value {
        "process" => Ok(RuntimeIsolationLevel::Process),
        "container" => Ok(RuntimeIsolationLevel::Container),
        "sandbox" => Ok(RuntimeIsolationLevel::Sandbox),
        "confidential" => Ok(RuntimeIsolationLevel::Confidential),
        _ => Err(format!("unsupported Function Runtime isolation {value:?}")),
    }
}
