use super::super::WorkflowDataType;
use crate::modules::shared_kernel::domain::Sha256Digest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowVariableScope {
    InvocationInput,
    NodeOutput,
    CompositeLocal,
    Run,
    Application,
}

impl WorkflowVariableScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvocationInput => "invocation_input",
            Self::NodeOutput => "node_output",
            Self::CompositeLocal => "composite_local",
            Self::Run => "run",
            Self::Application => "application",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "invocation_input" => Ok(Self::InvocationInput),
            "node_output" => Ok(Self::NodeOutput),
            "composite_local" => Ok(Self::CompositeLocal),
            "run" => Ok(Self::Run),
            "application" => Ok(Self::Application),
            _ => Err(format!("unsupported Workflow variable scope {value:?}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowVariableStorageClass {
    Inline,
    SecretReference,
    ImmutableObjectReference,
}

impl WorkflowVariableStorageClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::SecretReference => "secret_reference",
            Self::ImmutableObjectReference => "immutable_object_reference",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "inline" => Ok(Self::Inline),
            "secret_reference" => Ok(Self::SecretReference),
            "immutable_object_reference" => Ok(Self::ImmutableObjectReference),
            _ => Err(format!(
                "unsupported Workflow variable storage class {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowVariableMutationMode {
    Immutable,
    Deterministic,
    OptimisticApplicationPort,
}

impl WorkflowVariableMutationMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Immutable => "immutable",
            Self::Deterministic => "deterministic",
            Self::OptimisticApplicationPort => "optimistic_application_port",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "immutable" => Ok(Self::Immutable),
            "deterministic" => Ok(Self::Deterministic),
            "optimistic_application_port" => Ok(Self::OptimisticApplicationPort),
            _ => Err(format!(
                "unsupported Workflow variable mutation mode {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowVariableReadMode {
    DirectValue,
    OpaqueReference,
    ApplicationPort,
}

impl WorkflowVariableReadMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DirectValue => "direct_value",
            Self::OpaqueReference => "opaque_reference",
            Self::ApplicationPort => "application_port",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "direct_value" => Ok(Self::DirectValue),
            "opaque_reference" => Ok(Self::OpaqueReference),
            "application_port" => Ok(Self::ApplicationPort),
            _ => Err(format!("unsupported Workflow variable read mode {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowVariableDeclaration {
    pub name: String,
    pub scope: WorkflowVariableScope,
    pub value_type: WorkflowDataType,
    pub value_schema_digest: Sha256Digest,
    pub source_schema_digest: Option<Sha256Digest>,
    pub storage_class: WorkflowVariableStorageClass,
    pub mutation_mode: WorkflowVariableMutationMode,
    pub required: bool,
    pub source_step_id: Option<String>,
    pub source_path: Vec<String>,
    pub region_id: Option<String>,
    pub default_value_digest: Option<Sha256Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowVariableRead {
    pub id: String,
    pub variable: String,
    pub consumer_step_id: String,
    pub consumer_region_id: Option<String>,
    pub target_port: String,
    pub path: Vec<String>,
    pub expected_type: WorkflowDataType,
    pub expected_schema_digest: Sha256Digest,
    pub required: bool,
    pub mode: WorkflowVariableReadMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowVariableAssignment {
    pub id: String,
    pub target_variable: String,
    pub source_variable: String,
    pub writer_step_id: String,
    pub writer_region_id: Option<String>,
    pub source_path: Vec<String>,
    pub value_type: WorkflowDataType,
    pub value_schema_digest: Sha256Digest,
    pub mutation_order: u32,
    pub expected_revision_variable: Option<String>,
    pub idempotency_key_variable: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowVariableExport {
    pub id: String,
    pub region_id: String,
    pub source_variable: String,
    pub target_variable: String,
    pub source_path: Vec<String>,
    pub value_type: WorkflowDataType,
    pub value_schema_digest: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowVariableContractSpec {
    pub id: String,
    pub revision: String,
    pub compiler_schema_version: u32,
    pub declarations: Vec<WorkflowVariableDeclaration>,
    pub reads: Vec<WorkflowVariableRead>,
    pub assignments: Vec<WorkflowVariableAssignment>,
    pub exports: Vec<WorkflowVariableExport>,
}
