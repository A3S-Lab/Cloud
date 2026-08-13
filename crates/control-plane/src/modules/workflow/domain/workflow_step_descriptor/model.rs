use super::super::{CapabilityType, WorkflowDataType, WorkflowStepKind};
use crate::modules::shared_kernel::domain::Sha256Digest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepOwner {
    Workflow,
    Applications,
    Automations,
    Connectors,
    Files,
    Knowledge,
    Inference,
    Agents,
    Assets,
    Use,
    Executions,
    Sources,
}

impl WorkflowStepOwner {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Workflow => "workflow",
            Self::Applications => "applications",
            Self::Automations => "automations",
            Self::Connectors => "connectors",
            Self::Files => "files",
            Self::Knowledge => "knowledge",
            Self::Inference => "inference",
            Self::Agents => "agents",
            Self::Assets => "assets",
            Self::Use => "use",
            Self::Executions => "executions",
            Self::Sources => "sources",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "workflow" => Ok(Self::Workflow),
            "applications" => Ok(Self::Applications),
            "automations" => Ok(Self::Automations),
            "connectors" => Ok(Self::Connectors),
            "files" => Ok(Self::Files),
            "knowledge" => Ok(Self::Knowledge),
            "inference" => Ok(Self::Inference),
            "agents" => Ok(Self::Agents),
            "assets" => Ok(Self::Assets),
            "use" => Ok(Self::Use),
            "executions" => Ok(Self::Executions),
            "sources" => Ok(Self::Sources),
            _ => Err(format!("unsupported Workflow step owner {value:?}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepExecutionClass {
    WorkflowLocal,
    CompositeRegion,
    OwningApplicationPort,
    InvocationOnly,
}

impl WorkflowStepExecutionClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkflowLocal => "workflow_local",
            Self::CompositeRegion => "composite_region",
            Self::OwningApplicationPort => "owning_application_port",
            Self::InvocationOnly => "invocation_only",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "workflow_local" => Ok(Self::WorkflowLocal),
            "composite_region" => Ok(Self::CompositeRegion),
            "owning_application_port" => Ok(Self::OwningApplicationPort),
            "invocation_only" => Ok(Self::InvocationOnly),
            _ => Err(format!(
                "unsupported Workflow step execution class {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepPortCardinality {
    Single,
    Many,
}

impl WorkflowStepPortCardinality {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Many => "many",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "single" => Ok(Self::Single),
            "many" => Ok(Self::Many),
            _ => Err(format!("unsupported Workflow port cardinality {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStepPort {
    pub name: String,
    pub value_type: WorkflowDataType,
    pub cardinality: WorkflowStepPortCardinality,
    pub required: bool,
    pub dynamic: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepBindingKind {
    CapabilityReference,
    ReleaseReference,
    SecretReference,
    PlacementPolicy,
    EgressPolicy,
}

impl WorkflowStepBindingKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CapabilityReference => "capability_reference",
            Self::ReleaseReference => "release_reference",
            Self::SecretReference => "secret_reference",
            Self::PlacementPolicy => "placement_policy",
            Self::EgressPolicy => "egress_policy",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "capability_reference" => Ok(Self::CapabilityReference),
            "release_reference" => Ok(Self::ReleaseReference),
            "secret_reference" => Ok(Self::SecretReference),
            "placement_policy" => Ok(Self::PlacementPolicy),
            "egress_policy" => Ok(Self::EgressPolicy),
            _ => Err(format!("unsupported Workflow step binding {value:?}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepRetryClassification {
    NotRetryable,
    FlowRetryable,
    OwnerClassified,
}

impl WorkflowStepRetryClassification {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotRetryable => "not_retryable",
            Self::FlowRetryable => "flow_retryable",
            Self::OwnerClassified => "owner_classified",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "not_retryable" => Ok(Self::NotRetryable),
            "flow_retryable" => Ok(Self::FlowRetryable),
            "owner_classified" => Ok(Self::OwnerClassified),
            _ => Err(format!(
                "unsupported Workflow step retry classification {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepFallbackMode {
    Unsupported,
    DefaultOutput,
    FailureBranch,
}

impl WorkflowStepFallbackMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::DefaultOutput => "default_output",
            Self::FailureBranch => "failure_branch",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "unsupported" => Ok(Self::Unsupported),
            "default_output" => Ok(Self::DefaultOutput),
            "failure_branch" => Ok(Self::FailureBranch),
            _ => Err(format!("unsupported Workflow fallback mode {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStepFailureContract {
    pub error_output: Option<WorkflowStepPort>,
    pub retry_classification: WorkflowStepRetryClassification,
    pub fallback: WorkflowStepFallbackMode,
    pub failure_branch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepDescriptorAdmission {
    Admitted,
    Unavailable,
}

impl WorkflowStepDescriptorAdmission {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admitted => "admitted",
            Self::Unavailable => "unavailable",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "admitted" => Ok(Self::Admitted),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err(format!(
                "unsupported Workflow descriptor admission {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStepPresentationSpec {
    pub label: String,
    pub summary: String,
    pub icon_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStepDescriptorSpec {
    pub id: String,
    pub revision: String,
    pub owner: WorkflowStepOwner,
    pub kind: Option<WorkflowStepKind>,
    pub semantic_profile: String,
    pub execution_class: WorkflowStepExecutionClass,
    pub input_ports: Vec<WorkflowStepPort>,
    pub output_ports: Vec<WorkflowStepPort>,
    pub configuration_schema_digest: Sha256Digest,
    pub default_policy_digest: Option<Sha256Digest>,
    pub required_bindings: Vec<WorkflowStepBindingKind>,
    pub allowed_capability_types: Vec<CapabilityType>,
    pub failure: WorkflowStepFailureContract,
    pub minimum_compiler_schema_version: u32,
    pub maximum_compiler_schema_version: u32,
    pub admission: WorkflowStepDescriptorAdmission,
    pub unavailable_reason: Option<String>,
    pub presentation: WorkflowStepPresentationSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStepDescriptorRegistrySpec {
    pub id: String,
    pub revision: String,
    pub compiler_schema_version: u32,
    pub descriptors: Vec<WorkflowStepDescriptorSpec>,
}
