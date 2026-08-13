use super::validation::{validate_identifier, validate_text};
use super::WorkflowStepKind;
use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_acl::builder::{boolean, number, string, BlockBuilder};
use a3s_acl::{
    canonical_digest_with_schema, generate_acl, parse_acl, validate_document, AttributeSchema,
    Block, BlockSchema, Cardinality, Document, Schema, ValueSchema,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const WORKFLOW_CONFIGURATION_SCHEMA: &str = "cloud.workflow.configuration.v1";
pub const WORKFLOW_DATA_SCHEMA: &str = "cloud.workflow.data-schema.v1";
pub const WORKFLOW_POLICY_SCHEMA: &str = "cloud.workflow.policy.v1";
pub const WORKFLOW_PAYLOAD_MAX_ACL_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowPayloadKind {
    Configuration,
    DataSchema,
    Policy,
}

impl WorkflowPayloadKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::DataSchema => "data_schema",
            Self::Policy => "policy",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "configuration" => Ok(Self::Configuration),
            "data_schema" => Ok(Self::DataSchema),
            "policy" => Ok(Self::Policy),
            _ => Err(format!("unsupported Workflow payload kind {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowBranchRoute {
    pub handle: String,
    pub equals: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStepConfiguration {
    pub step_kind: WorkflowStepKind,
    pub template: Option<String>,
    pub selector: Option<String>,
    pub default_handle: Option<String>,
    pub message: Option<String>,
    pub details: Option<String>,
    pub expires_after_seconds: Option<u64>,
    pub routes: Vec<WorkflowBranchRoute>,
}

impl WorkflowStepConfiguration {
    pub fn empty(step_kind: WorkflowStepKind) -> Self {
        Self {
            step_kind,
            template: None,
            selector: None,
            default_handle: None,
            message: None,
            details: None,
            expires_after_seconds: None,
            routes: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let has_template = self.template.is_some();
        let has_branch =
            self.selector.is_some() || self.default_handle.is_some() || !self.routes.is_empty();
        let has_decision = self.message.is_some()
            || self.details.is_some()
            || self.expires_after_seconds.is_some();
        match self.step_kind {
            WorkflowStepKind::Input => {
                reject_extra_configuration(has_template || has_branch || has_decision, "input")
            }
            WorkflowStepKind::Output => {
                if has_branch || has_decision {
                    return Err("Workflow output configuration contains unrelated fields".into());
                }
                if let Some(template) = &self.template {
                    validate_template(template)?;
                }
                Ok(())
            }
            WorkflowStepKind::Transform => {
                if has_branch || has_decision {
                    return Err("Workflow transform configuration contains unrelated fields".into());
                }
                validate_template(
                    self.template
                        .as_deref()
                        .ok_or_else(|| "Workflow transform requires a template".to_owned())?,
                )
            }
            WorkflowStepKind::Branch => {
                if has_template || has_decision {
                    return Err("Workflow branch configuration contains unrelated fields".into());
                }
                validate_selector(
                    self.selector
                        .as_deref()
                        .ok_or_else(|| "Workflow branch requires a selector".to_owned())?,
                )?;
                validate_identifier(
                    "Workflow branch default handle",
                    self.default_handle
                        .as_deref()
                        .ok_or_else(|| "Workflow branch requires a default handle".to_owned())?,
                )?;
                if self.routes.is_empty() || self.routes.len() > 128 {
                    return Err("Workflow branch must declare between 1 and 128 routes".into());
                }
                let mut handles = BTreeSet::new();
                for route in &self.routes {
                    validate_identifier("Workflow branch route handle", &route.handle)?;
                    validate_text("Workflow branch route match", &route.equals, 0, 4_096)?;
                    if !handles.insert(route.handle.as_str()) {
                        return Err(format!(
                            "Workflow branch contains duplicate route handle {:?}",
                            route.handle
                        ));
                    }
                }
                Ok(())
            }
            WorkflowStepKind::HumanDecision => {
                if has_template || has_branch {
                    return Err(
                        "Workflow human-decision configuration contains unrelated fields".into(),
                    );
                }
                validate_text(
                    "Workflow human-decision message",
                    self.message
                        .as_deref()
                        .ok_or_else(|| "Workflow human-decision requires a message".to_owned())?,
                    1,
                    4_096,
                )?;
                if let Some(details) = &self.details {
                    validate_text("Workflow human-decision details", details, 0, 16 * 1024)?;
                }
                if self
                    .expires_after_seconds
                    .is_some_and(|seconds| seconds == 0 || seconds > 30 * 24 * 60 * 60)
                {
                    return Err(
                        "Workflow human-decision expiry must be between 1 second and 30 days"
                            .into(),
                    );
                }
                Ok(())
            }
            WorkflowStepKind::Execution
            | WorkflowStepKind::Agent
            | WorkflowStepKind::Mcp
            | WorkflowStepKind::Model
            | WorkflowStepKind::Tool
            | WorkflowStepKind::Service
            | WorkflowStepKind::Memory
            | WorkflowStepKind::Subworkflow => reject_extra_configuration(
                has_template || has_branch || has_decision,
                self.step_kind.as_str(),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowDataType {
    Any,
    Object,
    Array,
    String,
    Number,
    Boolean,
    Null,
}

impl WorkflowDataType {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Object => "object",
            Self::Array => "array",
            Self::String => "string",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Null => "null",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "any" => Ok(Self::Any),
            "object" => Ok(Self::Object),
            "array" => Ok(Self::Array),
            "string" => Ok(Self::String),
            "number" => Ok(Self::Number),
            "boolean" => Ok(Self::Boolean),
            "null" => Ok(Self::Null),
            _ => Err(format!("unsupported Workflow data type {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDataField {
    pub name: String,
    pub value_type: WorkflowDataType,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDataSchema {
    pub value_type: WorkflowDataType,
    pub fields: Vec<WorkflowDataField>,
}

impl WorkflowDataSchema {
    pub fn validate(&self) -> Result<(), String> {
        if self.fields.len() > 512 {
            return Err("Workflow data schema contains too many fields".into());
        }
        if self.value_type != WorkflowDataType::Object && !self.fields.is_empty() {
            return Err("Only Workflow object schemas may declare fields".into());
        }
        let mut names = BTreeSet::new();
        for field in &self.fields {
            validate_identifier("Workflow data field", &field.name)?;
            if !names.insert(field.name.as_str()) {
                return Err(format!(
                    "Workflow data schema contains duplicate field {:?}",
                    field.name
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowPolicyMode {
    Static,
    RecordedChoice,
}

impl WorkflowPolicyMode {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::RecordedChoice => "recorded_choice",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "static" => Ok(Self::Static),
            "recorded_choice" => Ok(Self::RecordedChoice),
            _ => Err(format!("unsupported Workflow policy mode {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPolicyCandidate {
    pub id: String,
    pub digest: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPolicy {
    pub mode: WorkflowPolicyMode,
    pub expression: Option<String>,
    pub candidates: Vec<WorkflowPolicyCandidate>,
}

impl WorkflowPolicy {
    pub fn validate(&self) -> Result<(), String> {
        match self.mode {
            WorkflowPolicyMode::Static
                if self.expression.is_none() && self.candidates.is_empty() =>
            {
                Ok(())
            }
            WorkflowPolicyMode::RecordedChoice => {
                validate_text(
                    "Workflow recorded-choice expression",
                    self.expression.as_deref().ok_or_else(|| {
                        "Workflow recorded-choice policy requires an expression".to_owned()
                    })?,
                    1,
                    4_096,
                )?;
                if self.candidates.len() < 2 || self.candidates.len() > 128 {
                    return Err(
                        "Workflow recorded-choice policy requires between 2 and 128 candidates"
                            .into(),
                    );
                }
                let mut ids = BTreeSet::new();
                for candidate in &self.candidates {
                    validate_identifier("Workflow policy candidate", &candidate.id)?;
                    if !ids.insert(candidate.id.as_str()) {
                        return Err(format!(
                            "Workflow policy contains duplicate candidate {:?}",
                            candidate.id
                        ));
                    }
                }
                Ok(())
            }
            WorkflowPolicyMode::Static => {
                Err("Workflow static policy cannot declare an expression or candidates".into())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum WorkflowPayloadContent {
    Configuration(WorkflowStepConfiguration),
    DataSchema(WorkflowDataSchema),
    Policy(WorkflowPolicy),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowPayload {
    kind: WorkflowPayloadKind,
    schema: String,
    canonical_acl: String,
    digest: Sha256Digest,
    content: WorkflowPayloadContent,
}

impl WorkflowPayload {
    pub fn parse_acl(kind: WorkflowPayloadKind, acl: &str) -> Result<Self, String> {
        if acl.is_empty() || acl.len() > WORKFLOW_PAYLOAD_MAX_ACL_BYTES {
            return Err("Workflow payload ACL size is invalid".into());
        }
        let document =
            parse_acl(acl).map_err(|error| format!("Workflow payload ACL is invalid: {error}"))?;
        let schema = payload_schema(kind)?;
        validate_closed_schema(&document, &schema)?;
        let root = document
            .blocks
            .first()
            .ok_or_else(|| "Workflow payload block is missing".to_owned())?;
        let declared_schema = required_string(root, "schema")?;
        if declared_schema != schema_name(kind) {
            return Err(format!(
                "Workflow {} payload schema is unsupported",
                kind.as_str()
            ));
        }
        let content = match kind {
            WorkflowPayloadKind::Configuration => {
                WorkflowPayloadContent::Configuration(parse_configuration(root)?)
            }
            WorkflowPayloadKind::DataSchema => {
                WorkflowPayloadContent::DataSchema(parse_data_schema(root)?)
            }
            WorkflowPayloadKind::Policy => WorkflowPayloadContent::Policy(parse_policy(root)?),
        };
        Self::from_content(content)
    }

    pub fn from_content(content: WorkflowPayloadContent) -> Result<Self, String> {
        validate_content(&content)?;
        let kind = content_kind(&content);
        let schema = payload_schema(kind)?;
        let document = payload_document(&content);
        let canonical_acl = generate_acl(&document);
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated Workflow payload ACL is invalid: {error}"))?;
        validate_closed_schema(&reparsed, &schema)?;
        let digest = Sha256Digest::parse(
            canonical_digest_with_schema(&reparsed, &schema)
                .map_err(|error| format!("Workflow payload is not canonicalizable: {error}"))?,
        )?;
        Ok(Self {
            kind,
            schema: schema_name(kind).to_owned(),
            canonical_acl,
            digest,
            content,
        })
    }

    pub fn restore(
        kind: WorkflowPayloadKind,
        acl: &str,
        stored_digest: &str,
    ) -> Result<Self, String> {
        let payload = Self::parse_acl(kind, acl)?;
        if payload.digest.as_str() != stored_digest {
            return Err("stored Workflow payload ACL and digest do not match".into());
        }
        Ok(payload)
    }

    pub const fn kind(&self) -> WorkflowPayloadKind {
        self.kind
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub const fn content(&self) -> &WorkflowPayloadContent {
        &self.content
    }
}

fn validate_content(content: &WorkflowPayloadContent) -> Result<(), String> {
    match content {
        WorkflowPayloadContent::Configuration(value) => value.validate(),
        WorkflowPayloadContent::DataSchema(value) => value.validate(),
        WorkflowPayloadContent::Policy(value) => value.validate(),
    }
}

const fn content_kind(content: &WorkflowPayloadContent) -> WorkflowPayloadKind {
    match content {
        WorkflowPayloadContent::Configuration(_) => WorkflowPayloadKind::Configuration,
        WorkflowPayloadContent::DataSchema(_) => WorkflowPayloadKind::DataSchema,
        WorkflowPayloadContent::Policy(_) => WorkflowPayloadKind::Policy,
    }
}

const fn schema_name(kind: WorkflowPayloadKind) -> &'static str {
    match kind {
        WorkflowPayloadKind::Configuration => WORKFLOW_CONFIGURATION_SCHEMA,
        WorkflowPayloadKind::DataSchema => WORKFLOW_DATA_SCHEMA,
        WorkflowPayloadKind::Policy => WORKFLOW_POLICY_SCHEMA,
    }
}

fn reject_extra_configuration(has_extra: bool, kind: &str) -> Result<(), String> {
    if has_extra {
        Err(format!(
            "Workflow {kind} configuration cannot declare local semantic fields"
        ))
    } else {
        Ok(())
    }
}

fn validate_template(value: &str) -> Result<(), String> {
    validate_text("Workflow template", value, 1, 64 * 1024)
}

fn validate_selector(value: &str) -> Result<(), String> {
    validate_text("Workflow branch selector", value, 1, 512)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err("Workflow branch selector must use a portable data path".into());
    }
    Ok(())
}

fn payload_schema(kind: WorkflowPayloadKind) -> Result<Schema, String> {
    match kind {
        WorkflowPayloadKind::Configuration => configuration_schema(),
        WorkflowPayloadKind::DataSchema => data_schema_schema(),
        WorkflowPayloadKind::Policy => policy_schema(),
    }
}

fn configuration_schema() -> Result<Schema, String> {
    let route = Schema::new().attribute("equals", AttributeSchema::required(ValueSchema::string()));
    let root = Schema::new()
        .attribute("schema", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "step_kind",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute("template", AttributeSchema::optional(ValueSchema::string()))
        .attribute("selector", AttributeSchema::optional(ValueSchema::string()))
        .attribute(
            "default_handle",
            AttributeSchema::optional(ValueSchema::string()),
        )
        .attribute("message", AttributeSchema::optional(ValueSchema::string()))
        .attribute("details", AttributeSchema::optional(ValueSchema::string()))
        .attribute(
            "expires_after_seconds",
            AttributeSchema::optional(ValueSchema::number()),
        )
        .block(
            "route",
            BlockSchema::new(route)
                .occurrences(
                    Cardinality::new(0, Some(128))
                        .map_err(|error| format!("Workflow route schema is invalid: {error}"))?,
                )
                .labels(Cardinality::exactly(1))
                .unordered(true),
        );
    Ok(Schema::new().block(
        "configuration",
        BlockSchema::new(root).occurrences(Cardinality::exactly(1)),
    ))
}

fn data_schema_schema() -> Result<Schema, String> {
    let field = Schema::new()
        .attribute(
            "value_type",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute("required", AttributeSchema::required(ValueSchema::bool()));
    let root =
        Schema::new()
            .attribute("schema", AttributeSchema::required(ValueSchema::string()))
            .attribute(
                "value_type",
                AttributeSchema::required(ValueSchema::string()),
            )
            .block(
                "field",
                BlockSchema::new(field)
                    .occurrences(Cardinality::new(0, Some(512)).map_err(|error| {
                        format!("Workflow data field schema is invalid: {error}")
                    })?)
                    .labels(Cardinality::exactly(1))
                    .unordered(true),
            );
    Ok(Schema::new().block(
        "data_schema",
        BlockSchema::new(root).occurrences(Cardinality::exactly(1)),
    ))
}

fn policy_schema() -> Result<Schema, String> {
    let candidate =
        Schema::new().attribute("digest", AttributeSchema::required(ValueSchema::string()));
    let root = Schema::new()
        .attribute("schema", AttributeSchema::required(ValueSchema::string()))
        .attribute("mode", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "expression",
            AttributeSchema::optional(ValueSchema::string()),
        )
        .block(
            "candidate",
            BlockSchema::new(candidate)
                .occurrences(Cardinality::new(0, Some(128)).map_err(|error| {
                    format!("Workflow policy candidate schema is invalid: {error}")
                })?)
                .labels(Cardinality::exactly(1))
                .unordered(true),
        );
    Ok(Schema::new().block(
        "policy",
        BlockSchema::new(root).occurrences(Cardinality::exactly(1)),
    ))
}

fn validate_closed_schema(document: &Document, schema: &Schema) -> Result<(), String> {
    let report = validate_document(document, schema);
    if report.is_empty() {
        return Ok(());
    }
    let diagnostics = report
        .diagnostics
        .iter()
        .take(8)
        .map(|item| format!("{} at {}", item.code, item.path))
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "Workflow payload ACL does not match its closed schema: {diagnostics}"
    ))
}

fn parse_configuration(root: &Block) -> Result<WorkflowStepConfiguration, String> {
    let expires_after_seconds = optional_number(root, "expires_after_seconds")?
        .map(positive_integer)
        .transpose()?;
    let mut routes = root
        .blocks
        .iter()
        .filter(|block| block.name == "route")
        .map(|block| {
            Ok(WorkflowBranchRoute {
                handle: required_label(block, "Workflow route")?,
                equals: required_string(block, "equals")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    routes.sort_by(|left, right| left.handle.cmp(&right.handle));
    let value = WorkflowStepConfiguration {
        step_kind: WorkflowStepKind::parse(&required_string(root, "step_kind")?)?,
        template: optional_string(root, "template")?,
        selector: optional_string(root, "selector")?,
        default_handle: optional_string(root, "default_handle")?,
        message: optional_string(root, "message")?,
        details: optional_string(root, "details")?,
        expires_after_seconds,
        routes,
    };
    value.validate()?;
    Ok(value)
}

fn parse_data_schema(root: &Block) -> Result<WorkflowDataSchema, String> {
    let mut fields = root
        .blocks
        .iter()
        .filter(|block| block.name == "field")
        .map(|block| {
            Ok(WorkflowDataField {
                name: required_label(block, "Workflow data field")?,
                value_type: WorkflowDataType::parse(&required_string(block, "value_type")?)?,
                required: required_bool(block, "required")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    fields.sort_by(|left, right| left.name.cmp(&right.name));
    let value = WorkflowDataSchema {
        value_type: WorkflowDataType::parse(&required_string(root, "value_type")?)?,
        fields,
    };
    value.validate()?;
    Ok(value)
}

fn parse_policy(root: &Block) -> Result<WorkflowPolicy, String> {
    let mut candidates = root
        .blocks
        .iter()
        .filter(|block| block.name == "candidate")
        .map(|block| {
            Ok(WorkflowPolicyCandidate {
                id: required_label(block, "Workflow policy candidate")?,
                digest: Sha256Digest::parse(required_string(block, "digest")?)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    candidates.sort_by(|left, right| left.id.cmp(&right.id));
    let value = WorkflowPolicy {
        mode: WorkflowPolicyMode::parse(&required_string(root, "mode")?)?,
        expression: optional_string(root, "expression")?,
        candidates,
    };
    value.validate()?;
    Ok(value)
}

fn payload_document(content: &WorkflowPayloadContent) -> Document {
    let root = match content {
        WorkflowPayloadContent::Configuration(value) => {
            let mut root = BlockBuilder::new("configuration")
                .attr("schema", string(WORKFLOW_CONFIGURATION_SCHEMA))
                .attr("step_kind", string(value.step_kind.as_str()));
            for (key, item) in [
                ("template", value.template.as_deref()),
                ("selector", value.selector.as_deref()),
                ("default_handle", value.default_handle.as_deref()),
                ("message", value.message.as_deref()),
                ("details", value.details.as_deref()),
            ] {
                if let Some(item) = item {
                    root = root.attr(key, string(item));
                }
            }
            if let Some(seconds) = value.expires_after_seconds {
                root = root.attr("expires_after_seconds", number(seconds as f64));
            }
            for route in &value.routes {
                root = root.nested_block(
                    BlockBuilder::new("route")
                        .label(&route.handle)
                        .attr("equals", string(&route.equals))
                        .build(),
                );
            }
            root.build()
        }
        WorkflowPayloadContent::DataSchema(value) => {
            let mut root = BlockBuilder::new("data_schema")
                .attr("schema", string(WORKFLOW_DATA_SCHEMA))
                .attr("value_type", string(value.value_type.as_str()));
            for field in &value.fields {
                root = root.nested_block(
                    BlockBuilder::new("field")
                        .label(&field.name)
                        .attr("value_type", string(field.value_type.as_str()))
                        .attr("required", boolean(field.required))
                        .build(),
                );
            }
            root.build()
        }
        WorkflowPayloadContent::Policy(value) => {
            let mut root = BlockBuilder::new("policy")
                .attr("schema", string(WORKFLOW_POLICY_SCHEMA))
                .attr("mode", string(value.mode.as_str()));
            if let Some(expression) = &value.expression {
                root = root.attr("expression", string(expression));
            }
            for candidate in &value.candidates {
                root = root.nested_block(
                    BlockBuilder::new("candidate")
                        .label(&candidate.id)
                        .attr("digest", string(candidate.digest.as_str()))
                        .build(),
                );
            }
            root.build()
        }
    };
    Document { blocks: vec![root] }
}

fn required_label(block: &Block, label: &str) -> Result<String, String> {
    block
        .labels
        .first()
        .cloned()
        .ok_or_else(|| format!("{label} label is missing"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    optional_string(block, name)?.ok_or_else(|| format!("{name} is missing"))
}

fn optional_string(block: &Block, name: &str) -> Result<Option<String>, String> {
    block
        .attributes
        .get(name)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{name} must be a string"))
        })
        .transpose()
}

fn required_bool(block: &Block, name: &str) -> Result<bool, String> {
    block
        .attributes
        .get(name)
        .and_then(|value| value.as_bool())
        .ok_or_else(|| format!("{name} must be a boolean"))
}

fn optional_number(block: &Block, name: &str) -> Result<Option<f64>, String> {
    block
        .attributes
        .get(name)
        .map(|value| {
            value
                .as_number()
                .ok_or_else(|| format!("{name} must be a number"))
        })
        .transpose()
}

fn positive_integer(value: f64) -> Result<u64, String> {
    if !value.is_finite() || value <= 0.0 || value.fract() != 0.0 || value > u64::MAX as f64 {
        return Err("Workflow payload number must be a positive integer".into());
    }
    Ok(value as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payloads_are_closed_canonical_and_digest_verified() {
        let acl = r#"
configuration {
  step_kind = "transform"
  template = "Hello {{input.name}}"
  schema = "cloud.workflow.configuration.v1"
}
"#;
        let payload =
            WorkflowPayload::parse_acl(WorkflowPayloadKind::Configuration, acl).expect("payload");
        assert_eq!(payload.kind(), WorkflowPayloadKind::Configuration);
        assert!(payload.digest().as_str().starts_with("sha256:"));
        assert_eq!(
            WorkflowPayload::restore(
                WorkflowPayloadKind::Configuration,
                payload.canonical_acl(),
                payload.digest().as_str(),
            )
            .expect("restore"),
            payload
        );
        assert!(WorkflowPayload::restore(
            WorkflowPayloadKind::Configuration,
            payload.canonical_acl(),
            &format!("sha256:{}", "0".repeat(64)),
        )
        .is_err());
        assert!(WorkflowPayload::parse_acl(
            WorkflowPayloadKind::Configuration,
            &acl.replace("template =", "unknown = \"x\"\n  template ="),
        )
        .is_err());
    }

    #[test]
    fn branch_candidates_and_human_expiry_fail_closed() {
        let branch = WorkflowStepConfiguration {
            step_kind: WorkflowStepKind::Branch,
            template: None,
            selector: Some("input.kind".into()),
            default_handle: Some("other".into()),
            message: None,
            details: None,
            expires_after_seconds: None,
            routes: vec![
                WorkflowBranchRoute {
                    handle: "fix".into(),
                    equals: "fix".into(),
                },
                WorkflowBranchRoute {
                    handle: "other".into(),
                    equals: "other".into(),
                },
            ],
        };
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(branch.clone()))
            .expect("branch");
        let mut duplicate = branch;
        duplicate.routes[1].handle = "fix".into();
        assert!(duplicate.validate().is_err());

        let mut decision = WorkflowStepConfiguration::empty(WorkflowStepKind::HumanDecision);
        decision.message = Some("Approve?".into());
        decision.expires_after_seconds = Some(30 * 24 * 60 * 60 + 1);
        assert!(decision.validate().is_err());
    }

    #[test]
    fn policy_records_dynamic_choice_inputs() {
        let policy = WorkflowPolicy {
            mode: WorkflowPolicyMode::RecordedChoice,
            expression: Some("input.priority".into()),
            candidates: vec![
                WorkflowPolicyCandidate {
                    id: "standard".into(),
                    digest: Sha256Digest::parse(format!("sha256:{}", "a".repeat(64)))
                        .expect("digest"),
                },
                WorkflowPolicyCandidate {
                    id: "urgent".into(),
                    digest: Sha256Digest::parse(format!("sha256:{}", "b".repeat(64)))
                        .expect("digest"),
                },
            ],
        };
        let payload =
            WorkflowPayload::from_content(WorkflowPayloadContent::Policy(policy)).expect("policy");
        assert_eq!(payload.schema(), WORKFLOW_POLICY_SCHEMA);
    }
}
