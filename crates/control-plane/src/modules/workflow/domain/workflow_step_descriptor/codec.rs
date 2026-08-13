use super::super::{CapabilityType, WorkflowDataType, WorkflowStepKind};
use super::model::{
    WorkflowStepBindingKind, WorkflowStepDescriptorAdmission, WorkflowStepDescriptorRegistrySpec,
    WorkflowStepDescriptorSpec, WorkflowStepExecutionClass, WorkflowStepFailureContract,
    WorkflowStepFallbackMode, WorkflowStepOwner, WorkflowStepPort, WorkflowStepPortCardinality,
    WorkflowStepPresentationSpec, WorkflowStepRetryClassification,
};
use super::{
    WORKFLOW_STEP_DESCRIPTOR_REGISTRY_SCHEMA, WORKFLOW_STEP_DESCRIPTOR_SEMANTIC_SCHEMA,
    WORKFLOW_STEP_PRESENTATION_SCHEMA,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_acl::builder::{boolean, integer, list, string, BlockBuilder};
use a3s_acl::{parse_acl, Block, Document, Value};

const REGISTRY_ATTRIBUTES: [&str; 3] = ["compiler_schema_version", "revision", "schema"];
const DESCRIPTOR_REQUIRED_ATTRIBUTES: [&str; 9] = [
    "admission",
    "allowed_capability_types",
    "configuration_schema_digest",
    "execution_class",
    "maximum_compiler_schema_version",
    "minimum_compiler_schema_version",
    "owner",
    "required_bindings",
    "semantic_profile",
];
const DESCRIPTOR_OPTIONAL_ATTRIBUTES: [&str; 3] =
    ["default_policy_digest", "kind", "unavailable_reason"];
const PORT_ATTRIBUTES: [&str; 4] = ["cardinality", "dynamic", "required", "value_type"];
const FAILURE_ATTRIBUTES: [&str; 3] = ["failure_branch", "fallback", "retry_classification"];
const PRESENTATION_ATTRIBUTES: [&str; 3] = ["icon_key", "label", "summary"];

pub(super) fn parse_registry_spec(
    source: &str,
) -> Result<WorkflowStepDescriptorRegistrySpec, String> {
    let document = parse_acl(source)
        .map_err(|error| format!("Workflow descriptor registry ACL is invalid: {error}"))?;
    if document.blocks.len() != 1 {
        return Err("Workflow descriptor registry requires exactly one root block".into());
    }
    let root = &document.blocks[0];
    exact_block(
        root,
        "descriptor_registry",
        &REGISTRY_ATTRIBUTES,
        &[],
        1,
        true,
    )?;
    if required_string(root, "schema")? != WORKFLOW_STEP_DESCRIPTOR_REGISTRY_SCHEMA {
        return Err("Workflow descriptor registry schema is unsupported".into());
    }
    if root.blocks.iter().any(|block| block.name != "descriptor") {
        return Err("Workflow descriptor registry contains an unknown block".into());
    }
    Ok(WorkflowStepDescriptorRegistrySpec {
        id: root.labels[0].clone(),
        revision: required_string(root, "revision")?,
        compiler_schema_version: required_u32(root, "compiler_schema_version")?,
        descriptors: root
            .blocks
            .iter()
            .map(parse_descriptor)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub(super) fn registry_document(spec: &WorkflowStepDescriptorRegistrySpec) -> Document {
    let mut root = BlockBuilder::new("descriptor_registry")
        .label(&spec.id)
        .attr("schema", string(WORKFLOW_STEP_DESCRIPTOR_REGISTRY_SCHEMA))
        .attr("revision", string(&spec.revision))
        .attr(
            "compiler_schema_version",
            integer(i64::from(spec.compiler_schema_version)),
        );
    for descriptor in &spec.descriptors {
        root = root.nested_block(descriptor_block(descriptor, true));
    }
    Document {
        blocks: vec![root.build()],
    }
}

pub(super) fn semantic_document(spec: &WorkflowStepDescriptorSpec) -> Document {
    let mut root = BlockBuilder::new("step_descriptor")
        .label(&spec.id)
        .label(&spec.revision)
        .attr("schema", string(WORKFLOW_STEP_DESCRIPTOR_SEMANTIC_SCHEMA));
    root = append_semantic_attributes(root, spec);
    root = append_semantic_blocks(root, spec);
    Document {
        blocks: vec![root.build()],
    }
}

pub(super) fn presentation_document(
    descriptor_id: &str,
    descriptor_revision: &str,
    spec: &WorkflowStepPresentationSpec,
) -> Document {
    Document {
        blocks: vec![BlockBuilder::new("step_presentation")
            .label(descriptor_id)
            .label(descriptor_revision)
            .attr("schema", string(WORKFLOW_STEP_PRESENTATION_SCHEMA))
            .attr("label", string(&spec.label))
            .attr("summary", string(&spec.summary))
            .attr("icon_key", string(&spec.icon_key))
            .build()],
    }
}

fn descriptor_block(spec: &WorkflowStepDescriptorSpec, include_metadata: bool) -> Block {
    let mut block = BlockBuilder::new("descriptor")
        .label(&spec.id)
        .label(&spec.revision);
    block = append_semantic_attributes(block, spec);
    if include_metadata {
        block = block.attr("admission", string(spec.admission.as_str()));
        if let Some(reason) = &spec.unavailable_reason {
            block = block.attr("unavailable_reason", string(reason));
        }
    }
    block = append_semantic_blocks(block, spec);
    if include_metadata {
        block = block.nested_block(presentation_block(&spec.presentation));
    }
    block.build()
}

fn append_semantic_attributes(
    mut block: BlockBuilder,
    spec: &WorkflowStepDescriptorSpec,
) -> BlockBuilder {
    block = block
        .attr("owner", string(spec.owner.as_str()))
        .attr("semantic_profile", string(&spec.semantic_profile))
        .attr("execution_class", string(spec.execution_class.as_str()))
        .attr(
            "configuration_schema_digest",
            string(spec.configuration_schema_digest.as_str()),
        )
        .attr(
            "required_bindings",
            list(
                spec.required_bindings
                    .iter()
                    .map(|binding| string(binding.as_str()))
                    .collect(),
            ),
        )
        .attr(
            "allowed_capability_types",
            list(
                spec.allowed_capability_types
                    .iter()
                    .map(|capability_type| string(capability_type.as_str()))
                    .collect(),
            ),
        )
        .attr(
            "minimum_compiler_schema_version",
            integer(i64::from(spec.minimum_compiler_schema_version)),
        )
        .attr(
            "maximum_compiler_schema_version",
            integer(i64::from(spec.maximum_compiler_schema_version)),
        );
    if let Some(kind) = spec.kind {
        block = block.attr("kind", string(kind.as_str()));
    }
    if let Some(digest) = &spec.default_policy_digest {
        block = block.attr("default_policy_digest", string(digest.as_str()));
    }
    block
}

fn append_semantic_blocks(
    mut block: BlockBuilder,
    spec: &WorkflowStepDescriptorSpec,
) -> BlockBuilder {
    for port in &spec.input_ports {
        block = block.nested_block(port_block("input", port));
    }
    for port in &spec.output_ports {
        block = block.nested_block(port_block("output", port));
    }
    if let Some(port) = &spec.failure.error_output {
        block = block.nested_block(port_block("error_output", port));
    }
    block.nested_block(failure_block(&spec.failure))
}

fn port_block(name: &str, port: &WorkflowStepPort) -> Block {
    BlockBuilder::new(name)
        .label(&port.name)
        .attr("value_type", string(port.value_type.as_str()))
        .attr("cardinality", string(port.cardinality.as_str()))
        .attr("required", boolean(port.required))
        .attr("dynamic", boolean(port.dynamic))
        .build()
}

fn failure_block(failure: &WorkflowStepFailureContract) -> Block {
    BlockBuilder::new("failure")
        .attr(
            "retry_classification",
            string(failure.retry_classification.as_str()),
        )
        .attr("fallback", string(failure.fallback.as_str()))
        .attr("failure_branch", boolean(failure.failure_branch))
        .build()
}

fn presentation_block(presentation: &WorkflowStepPresentationSpec) -> Block {
    BlockBuilder::new("presentation")
        .attr("label", string(&presentation.label))
        .attr("summary", string(&presentation.summary))
        .attr("icon_key", string(&presentation.icon_key))
        .build()
}

fn parse_descriptor(block: &Block) -> Result<WorkflowStepDescriptorSpec, String> {
    exact_block(
        block,
        "descriptor",
        &DESCRIPTOR_REQUIRED_ATTRIBUTES,
        &DESCRIPTOR_OPTIONAL_ATTRIBUTES,
        2,
        true,
    )?;
    let failure = exact_nested(block, "failure")?;
    exact_block(failure, "failure", &FAILURE_ATTRIBUTES, &[], 0, false)?;
    let presentation = exact_nested(block, "presentation")?;
    exact_block(
        presentation,
        "presentation",
        &PRESENTATION_ATTRIBUTES,
        &[],
        0,
        false,
    )?;
    let error_outputs = block
        .blocks
        .iter()
        .filter(|nested| nested.name == "error_output")
        .collect::<Vec<_>>();
    if error_outputs.len() > 1
        || block.blocks.iter().any(|nested| {
            !matches!(
                nested.name.as_str(),
                "input" | "output" | "error_output" | "failure" | "presentation"
            )
        })
    {
        return Err("Workflow descriptor contains invalid nested blocks".into());
    }
    Ok(WorkflowStepDescriptorSpec {
        id: block.labels[0].clone(),
        revision: block.labels[1].clone(),
        owner: WorkflowStepOwner::parse(&required_string(block, "owner")?)?,
        kind: optional_string(block, "kind")?
            .map(|value| WorkflowStepKind::parse(&value))
            .transpose()?,
        semantic_profile: required_string(block, "semantic_profile")?,
        execution_class: WorkflowStepExecutionClass::parse(&required_string(
            block,
            "execution_class",
        )?)?,
        input_ports: parse_ports(block, "input")?,
        output_ports: parse_ports(block, "output")?,
        configuration_schema_digest: parse_digest(block, "configuration_schema_digest")?,
        default_policy_digest: optional_string(block, "default_policy_digest")?
            .map(Sha256Digest::parse)
            .transpose()?,
        required_bindings: required_strings(block, "required_bindings")?
            .into_iter()
            .map(|value| WorkflowStepBindingKind::parse(&value))
            .collect::<Result<Vec<_>, _>>()?,
        allowed_capability_types: required_strings(block, "allowed_capability_types")?
            .into_iter()
            .map(|value| CapabilityType::parse(&value))
            .collect::<Result<Vec<_>, _>>()?,
        failure: WorkflowStepFailureContract {
            error_output: error_outputs
                .first()
                .map(|port| parse_port(port))
                .transpose()?,
            retry_classification: WorkflowStepRetryClassification::parse(&required_string(
                failure,
                "retry_classification",
            )?)?,
            fallback: WorkflowStepFallbackMode::parse(&required_string(failure, "fallback")?)?,
            failure_branch: required_bool(failure, "failure_branch")?,
        },
        minimum_compiler_schema_version: required_u32(block, "minimum_compiler_schema_version")?,
        maximum_compiler_schema_version: required_u32(block, "maximum_compiler_schema_version")?,
        admission: WorkflowStepDescriptorAdmission::parse(&required_string(block, "admission")?)?,
        unavailable_reason: optional_string(block, "unavailable_reason")?,
        presentation: WorkflowStepPresentationSpec {
            label: required_string(presentation, "label")?,
            summary: required_string(presentation, "summary")?,
            icon_key: required_string(presentation, "icon_key")?,
        },
    })
}

fn parse_ports(block: &Block, name: &str) -> Result<Vec<WorkflowStepPort>, String> {
    block
        .blocks
        .iter()
        .filter(|nested| nested.name == name)
        .map(parse_port)
        .collect()
}

fn parse_port(block: &Block) -> Result<WorkflowStepPort, String> {
    exact_block(block, &block.name, &PORT_ATTRIBUTES, &[], 1, false)?;
    Ok(WorkflowStepPort {
        name: block.labels[0].clone(),
        value_type: parse_data_type(&required_string(block, "value_type")?)?,
        cardinality: WorkflowStepPortCardinality::parse(&required_string(block, "cardinality")?)?,
        required: required_bool(block, "required")?,
        dynamic: required_bool(block, "dynamic")?,
    })
}

fn parse_data_type(value: &str) -> Result<WorkflowDataType, String> {
    WorkflowDataType::parse(value)
}

fn exact_nested<'a>(root: &'a Block, name: &str) -> Result<&'a Block, String> {
    let mut matches = root.blocks.iter().filter(|block| block.name == name);
    let value = matches
        .next()
        .ok_or_else(|| format!("Workflow descriptor {name} block is required"))?;
    if matches.next().is_some() {
        return Err(format!("Workflow descriptor {name} block must be unique"));
    }
    Ok(value)
}

fn exact_block(
    block: &Block,
    name: &str,
    required: &[&str],
    optional: &[&str],
    labels: usize,
    allow_nested: bool,
) -> Result<(), String> {
    if block.name != name
        || block.labels.len() != labels
        || block.attributes.len() < required.len()
        || block.attributes.len() > required.len() + optional.len()
        || required
            .iter()
            .any(|name| !block.attributes.contains_key(*name))
        || block
            .attributes
            .keys()
            .any(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
        || (!allow_nested && !block.blocks.is_empty())
    {
        return Err(format!("Workflow descriptor {name} block shape is invalid"));
    }
    Ok(())
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("Workflow descriptor field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("Workflow descriptor field {name:?} must be a string"))
}

fn optional_string(block: &Block, name: &str) -> Result<Option<String>, String> {
    block
        .attributes
        .get(name)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("Workflow descriptor field {name:?} must be a string"))
        })
        .transpose()
}

fn required_strings(block: &Block, name: &str) -> Result<Vec<String>, String> {
    let Value::List(values) = required_value(block, name)? else {
        return Err(format!(
            "Workflow descriptor field {name:?} must be a string list"
        ));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("Workflow descriptor field {name:?} must be a string list"))
        })
        .collect()
}

fn required_bool(block: &Block, name: &str) -> Result<bool, String> {
    required_value(block, name)?
        .as_bool()
        .ok_or_else(|| format!("Workflow descriptor field {name:?} must be a boolean"))
}

fn required_u32(block: &Block, name: &str) -> Result<u32, String> {
    let value = required_value(block, name)?
        .as_number()
        .ok_or_else(|| format!("Workflow descriptor field {name:?} must be an integer"))?;
    if !value.is_finite() || value.fract() != 0.0 || value <= 0.0 || value > f64::from(u32::MAX) {
        return Err(format!(
            "Workflow descriptor field {name:?} must be a positive u32"
        ));
    }
    Ok(value as u32)
}

fn parse_digest(block: &Block, name: &str) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(required_string(block, name)?)
}
