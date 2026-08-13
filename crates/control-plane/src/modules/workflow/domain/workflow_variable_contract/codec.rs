use super::super::WorkflowDataType;
use super::model::{
    WorkflowVariableAssignment, WorkflowVariableContractSpec, WorkflowVariableDeclaration,
    WorkflowVariableExport, WorkflowVariableMutationMode, WorkflowVariableRead,
    WorkflowVariableReadMode, WorkflowVariableScope, WorkflowVariableStorageClass,
};
use super::WORKFLOW_VARIABLE_CONTRACT_SCHEMA;
use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_acl::builder::{boolean, integer, list, string, BlockBuilder};
use a3s_acl::{parse_acl, Block, Document, Value};

const ROOT_ATTRIBUTES: [&str; 3] = ["compiler_schema_version", "revision", "schema"];
const DECLARATION_REQUIRED: [&str; 7] = [
    "mutation_mode",
    "required",
    "scope",
    "source_path",
    "storage_class",
    "value_schema_digest",
    "value_type",
];
const DECLARATION_OPTIONAL: [&str; 4] = [
    "default_value_digest",
    "region_id",
    "source_schema_digest",
    "source_step_id",
];
const READ_REQUIRED: [&str; 8] = [
    "consumer_step_id",
    "expected_schema_digest",
    "expected_type",
    "mode",
    "path",
    "required",
    "target_port",
    "variable",
];
const READ_OPTIONAL: [&str; 1] = ["consumer_region_id"];
const ASSIGNMENT_REQUIRED: [&str; 7] = [
    "mutation_order",
    "source_path",
    "source_variable",
    "target_variable",
    "value_schema_digest",
    "value_type",
    "writer_step_id",
];
const ASSIGNMENT_OPTIONAL: [&str; 3] = [
    "expected_revision_variable",
    "idempotency_key_variable",
    "writer_region_id",
];
const EXPORT_REQUIRED: [&str; 6] = [
    "region_id",
    "source_path",
    "source_variable",
    "target_variable",
    "value_schema_digest",
    "value_type",
];

pub(super) fn parse_contract_spec(source: &str) -> Result<WorkflowVariableContractSpec, String> {
    let document = parse_acl(source)
        .map_err(|error| format!("Workflow variable contract ACL is invalid: {error}"))?;
    if document.blocks.len() != 1 {
        return Err("Workflow variable contract requires exactly one root block".into());
    }
    let root = &document.blocks[0];
    exact_block(root, "variable_contract", &ROOT_ATTRIBUTES, &[], 1, true)?;
    if required_string(root, "schema")? != WORKFLOW_VARIABLE_CONTRACT_SCHEMA {
        return Err("Workflow variable contract schema is unsupported".into());
    }
    if root.blocks.iter().any(|block| {
        !matches!(
            block.name.as_str(),
            "declaration" | "read" | "assignment" | "export"
        )
    }) {
        return Err("Workflow variable contract contains an unknown block".into());
    }
    Ok(WorkflowVariableContractSpec {
        id: root.labels[0].clone(),
        revision: required_string(root, "revision")?,
        compiler_schema_version: required_u32(root, "compiler_schema_version")?,
        declarations: root
            .blocks
            .iter()
            .filter(|block| block.name == "declaration")
            .map(parse_declaration)
            .collect::<Result<Vec<_>, _>>()?,
        reads: root
            .blocks
            .iter()
            .filter(|block| block.name == "read")
            .map(parse_read)
            .collect::<Result<Vec<_>, _>>()?,
        assignments: root
            .blocks
            .iter()
            .filter(|block| block.name == "assignment")
            .map(parse_assignment)
            .collect::<Result<Vec<_>, _>>()?,
        exports: root
            .blocks
            .iter()
            .filter(|block| block.name == "export")
            .map(parse_export)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub(super) fn contract_document(spec: &WorkflowVariableContractSpec) -> Document {
    let mut root = BlockBuilder::new("variable_contract")
        .label(&spec.id)
        .attr("schema", string(WORKFLOW_VARIABLE_CONTRACT_SCHEMA))
        .attr("revision", string(&spec.revision))
        .attr(
            "compiler_schema_version",
            integer(i64::from(spec.compiler_schema_version)),
        );
    for declaration in &spec.declarations {
        root = root.nested_block(declaration_block(declaration));
    }
    for read in &spec.reads {
        root = root.nested_block(read_block(read));
    }
    for assignment in &spec.assignments {
        root = root.nested_block(assignment_block(assignment));
    }
    for export in &spec.exports {
        root = root.nested_block(export_block(export));
    }
    Document {
        blocks: vec![root.build()],
    }
}

fn declaration_block(value: &WorkflowVariableDeclaration) -> Block {
    let mut block = BlockBuilder::new("declaration")
        .label(&value.name)
        .attr("scope", string(value.scope.as_str()))
        .attr("value_type", string(value.value_type.as_str()))
        .attr(
            "value_schema_digest",
            string(value.value_schema_digest.as_str()),
        )
        .attr("storage_class", string(value.storage_class.as_str()))
        .attr("mutation_mode", string(value.mutation_mode.as_str()))
        .attr("required", boolean(value.required))
        .attr("source_path", string_list(&value.source_path));
    if let Some(source_step_id) = &value.source_step_id {
        block = block.attr("source_step_id", string(source_step_id));
    }
    if let Some(source_schema_digest) = &value.source_schema_digest {
        block = block.attr(
            "source_schema_digest",
            string(source_schema_digest.as_str()),
        );
    }
    if let Some(region_id) = &value.region_id {
        block = block.attr("region_id", string(region_id));
    }
    if let Some(default_value_digest) = &value.default_value_digest {
        block = block.attr(
            "default_value_digest",
            string(default_value_digest.as_str()),
        );
    }
    block.build()
}

fn read_block(value: &WorkflowVariableRead) -> Block {
    let mut block = BlockBuilder::new("read")
        .label(&value.id)
        .attr("variable", string(&value.variable))
        .attr("consumer_step_id", string(&value.consumer_step_id))
        .attr("target_port", string(&value.target_port))
        .attr("path", string_list(&value.path))
        .attr("expected_type", string(value.expected_type.as_str()))
        .attr(
            "expected_schema_digest",
            string(value.expected_schema_digest.as_str()),
        )
        .attr("required", boolean(value.required))
        .attr("mode", string(value.mode.as_str()));
    if let Some(region_id) = &value.consumer_region_id {
        block = block.attr("consumer_region_id", string(region_id));
    }
    block.build()
}

fn assignment_block(value: &WorkflowVariableAssignment) -> Block {
    let mut block = BlockBuilder::new("assignment")
        .label(&value.id)
        .attr("target_variable", string(&value.target_variable))
        .attr("source_variable", string(&value.source_variable))
        .attr("writer_step_id", string(&value.writer_step_id))
        .attr("source_path", string_list(&value.source_path))
        .attr("value_type", string(value.value_type.as_str()))
        .attr(
            "value_schema_digest",
            string(value.value_schema_digest.as_str()),
        )
        .attr("mutation_order", integer(i64::from(value.mutation_order)));
    if let Some(region_id) = &value.writer_region_id {
        block = block.attr("writer_region_id", string(region_id));
    }
    if let Some(variable) = &value.expected_revision_variable {
        block = block.attr("expected_revision_variable", string(variable));
    }
    if let Some(variable) = &value.idempotency_key_variable {
        block = block.attr("idempotency_key_variable", string(variable));
    }
    block.build()
}

fn export_block(value: &WorkflowVariableExport) -> Block {
    BlockBuilder::new("export")
        .label(&value.id)
        .attr("region_id", string(&value.region_id))
        .attr("source_variable", string(&value.source_variable))
        .attr("target_variable", string(&value.target_variable))
        .attr("source_path", string_list(&value.source_path))
        .attr("value_type", string(value.value_type.as_str()))
        .attr(
            "value_schema_digest",
            string(value.value_schema_digest.as_str()),
        )
        .build()
}

fn parse_declaration(block: &Block) -> Result<WorkflowVariableDeclaration, String> {
    exact_block(
        block,
        "declaration",
        &DECLARATION_REQUIRED,
        &DECLARATION_OPTIONAL,
        1,
        false,
    )?;
    Ok(WorkflowVariableDeclaration {
        name: block.labels[0].clone(),
        scope: WorkflowVariableScope::parse(&required_string(block, "scope")?)?,
        value_type: WorkflowDataType::parse(&required_string(block, "value_type")?)?,
        value_schema_digest: parse_digest(block, "value_schema_digest")?,
        source_schema_digest: optional_string(block, "source_schema_digest")?
            .map(Sha256Digest::parse)
            .transpose()?,
        storage_class: WorkflowVariableStorageClass::parse(&required_string(
            block,
            "storage_class",
        )?)?,
        mutation_mode: WorkflowVariableMutationMode::parse(&required_string(
            block,
            "mutation_mode",
        )?)?,
        required: required_bool(block, "required")?,
        source_step_id: optional_string(block, "source_step_id")?,
        source_path: required_strings(block, "source_path")?,
        region_id: optional_string(block, "region_id")?,
        default_value_digest: optional_string(block, "default_value_digest")?
            .map(Sha256Digest::parse)
            .transpose()?,
    })
}

fn parse_read(block: &Block) -> Result<WorkflowVariableRead, String> {
    exact_block(block, "read", &READ_REQUIRED, &READ_OPTIONAL, 1, false)?;
    Ok(WorkflowVariableRead {
        id: block.labels[0].clone(),
        variable: required_string(block, "variable")?,
        consumer_step_id: required_string(block, "consumer_step_id")?,
        consumer_region_id: optional_string(block, "consumer_region_id")?,
        target_port: required_string(block, "target_port")?,
        path: required_strings(block, "path")?,
        expected_type: WorkflowDataType::parse(&required_string(block, "expected_type")?)?,
        expected_schema_digest: parse_digest(block, "expected_schema_digest")?,
        required: required_bool(block, "required")?,
        mode: WorkflowVariableReadMode::parse(&required_string(block, "mode")?)?,
    })
}

fn parse_assignment(block: &Block) -> Result<WorkflowVariableAssignment, String> {
    exact_block(
        block,
        "assignment",
        &ASSIGNMENT_REQUIRED,
        &ASSIGNMENT_OPTIONAL,
        1,
        false,
    )?;
    Ok(WorkflowVariableAssignment {
        id: block.labels[0].clone(),
        target_variable: required_string(block, "target_variable")?,
        source_variable: required_string(block, "source_variable")?,
        writer_step_id: required_string(block, "writer_step_id")?,
        writer_region_id: optional_string(block, "writer_region_id")?,
        source_path: required_strings(block, "source_path")?,
        value_type: WorkflowDataType::parse(&required_string(block, "value_type")?)?,
        value_schema_digest: parse_digest(block, "value_schema_digest")?,
        mutation_order: required_u32(block, "mutation_order")?,
        expected_revision_variable: optional_string(block, "expected_revision_variable")?,
        idempotency_key_variable: optional_string(block, "idempotency_key_variable")?,
    })
}

fn parse_export(block: &Block) -> Result<WorkflowVariableExport, String> {
    exact_block(block, "export", &EXPORT_REQUIRED, &[], 1, false)?;
    Ok(WorkflowVariableExport {
        id: block.labels[0].clone(),
        region_id: required_string(block, "region_id")?,
        source_variable: required_string(block, "source_variable")?,
        target_variable: required_string(block, "target_variable")?,
        source_path: required_strings(block, "source_path")?,
        value_type: WorkflowDataType::parse(&required_string(block, "value_type")?)?,
        value_schema_digest: parse_digest(block, "value_schema_digest")?,
    })
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
        return Err(format!("Workflow variable {name} block shape is invalid"));
    }
    Ok(())
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("Workflow variable field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("Workflow variable field {name:?} must be a string"))
}

fn optional_string(block: &Block, name: &str) -> Result<Option<String>, String> {
    block
        .attributes
        .get(name)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("Workflow variable field {name:?} must be a string"))
        })
        .transpose()
}

fn required_strings(block: &Block, name: &str) -> Result<Vec<String>, String> {
    let Value::List(values) = required_value(block, name)? else {
        return Err(format!(
            "Workflow variable field {name:?} must be a string list"
        ));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("Workflow variable field {name:?} must be a string list"))
        })
        .collect()
}

fn required_bool(block: &Block, name: &str) -> Result<bool, String> {
    required_value(block, name)?
        .as_bool()
        .ok_or_else(|| format!("Workflow variable field {name:?} must be a boolean"))
}

fn required_u32(block: &Block, name: &str) -> Result<u32, String> {
    let value = required_value(block, name)?
        .as_number()
        .ok_or_else(|| format!("Workflow variable field {name:?} must be an integer"))?;
    if !value.is_finite() || value.fract() != 0.0 || value <= 0.0 || value > f64::from(u32::MAX) {
        return Err(format!(
            "Workflow variable field {name:?} must be a positive u32"
        ));
    }
    Ok(value as u32)
}

fn parse_digest(block: &Block, name: &str) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(required_string(block, name)?)
}

fn string_list(values: &[String]) -> Value {
    list(values.iter().map(|value| string(value)).collect())
}
