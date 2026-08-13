use super::super::validation::{
    validate_dotted_identifier, validate_exact_semver, validate_identifier,
};
use super::super::WorkflowDataType;
use super::model::{
    WorkflowVariableAssignment, WorkflowVariableContractSpec, WorkflowVariableDeclaration,
    WorkflowVariableExport, WorkflowVariableMutationMode, WorkflowVariableRead,
    WorkflowVariableReadMode, WorkflowVariableScope, WorkflowVariableStorageClass,
};
use super::WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION;
use std::collections::{BTreeMap, BTreeSet};

const MAX_DECLARATIONS: usize = 1_024;
const MAX_READS: usize = 8_192;
const MAX_ASSIGNMENTS: usize = 8_192;
const MAX_EXPORTS: usize = 2_048;
const MAX_PATH_SEGMENTS: usize = 32;

pub(super) fn normalize_contract_spec(
    mut spec: WorkflowVariableContractSpec,
) -> Result<WorkflowVariableContractSpec, String> {
    validate_dotted_identifier("Workflow variable contract ID", &spec.id)?;
    validate_exact_semver("Workflow variable contract revision", &spec.revision)?;
    if spec.compiler_schema_version != WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION
        || spec.declarations.is_empty()
        || spec.declarations.len() > MAX_DECLARATIONS
        || spec.reads.len() > MAX_READS
        || spec.assignments.len() > MAX_ASSIGNMENTS
        || spec.exports.len() > MAX_EXPORTS
    {
        return Err("Workflow variable contract bounds are invalid".into());
    }

    for declaration in &spec.declarations {
        validate_declaration(declaration)?;
    }
    spec.declarations
        .sort_by(|left, right| left.name.cmp(&right.name));
    reject_duplicate_keys(
        spec.declarations.iter().map(|value| value.name.as_str()),
        "declaration",
    )?;
    let declarations = spec
        .declarations
        .iter()
        .map(|value| (value.name.as_str(), value))
        .collect::<BTreeMap<_, _>>();

    for read in &spec.reads {
        validate_read(read, &declarations)?;
    }
    spec.reads.sort_by(|left, right| left.id.cmp(&right.id));
    reject_duplicate_keys(spec.reads.iter().map(|value| value.id.as_str()), "read")?;
    reject_duplicate_read_targets(&spec.reads)?;

    for assignment in &spec.assignments {
        validate_assignment(assignment, &declarations)?;
    }
    spec.assignments.sort_by(|left, right| {
        left.target_variable
            .cmp(&right.target_variable)
            .then_with(|| left.mutation_order.cmp(&right.mutation_order))
            .then_with(|| left.id.cmp(&right.id))
    });
    reject_duplicate_keys(
        spec.assignments.iter().map(|value| value.id.as_str()),
        "assignment",
    )?;
    validate_mutation_order(&spec.assignments)?;

    for export in &spec.exports {
        validate_export(export, &declarations)?;
    }
    spec.exports.sort_by(|left, right| left.id.cmp(&right.id));
    reject_duplicate_keys(spec.exports.iter().map(|value| value.id.as_str()), "export")?;
    reject_duplicate_keys(
        spec.exports
            .iter()
            .map(|value| value.target_variable.as_str()),
        "export target",
    )?;
    Ok(spec)
}

fn validate_declaration(value: &WorkflowVariableDeclaration) -> Result<(), String> {
    validate_identifier("Workflow variable name", &value.name)?;
    validate_path("Workflow variable source path", &value.source_path)?;
    if let Some(step_id) = &value.source_step_id {
        validate_identifier("Workflow variable source step", step_id)?;
    }
    if let Some(region_id) = &value.region_id {
        validate_identifier("Workflow variable region", region_id)?;
    }
    if value.storage_class != WorkflowVariableStorageClass::Inline
        && value.value_type != WorkflowDataType::Object
    {
        return Err("Secret and immutable-object variables must be typed object references".into());
    }
    if value.storage_class != WorkflowVariableStorageClass::Inline
        && value.default_value_digest.is_some()
    {
        return Err("Opaque Workflow references cannot declare inline defaults".into());
    }
    if value.required && value.default_value_digest.is_some() {
        return Err("Required Workflow variables cannot declare a default value".into());
    }
    match value.scope {
        WorkflowVariableScope::InvocationInput => {
            require_origin(value, None, None, WorkflowVariableMutationMode::Immutable)?;
            require_source_schema(value)?;
        }
        WorkflowVariableScope::NodeOutput => {
            if value.source_step_id.is_none() || value.region_id.is_some() {
                return Err(
                    "Node-output variables require one source step and no local region".into(),
                );
            }
            if value.mutation_mode != WorkflowVariableMutationMode::Immutable {
                return Err("Node-output variables are immutable attempt results".into());
            }
            if value.default_value_digest.is_some() {
                return Err("Node-output variables cannot invent a default result".into());
            }
            require_source_schema(value)?;
        }
        WorkflowVariableScope::CompositeLocal => {
            if value.region_id.is_none()
                || value.mutation_mode == WorkflowVariableMutationMode::OptimisticApplicationPort
            {
                return Err(
                    "Composite-local variables require one region and local mutation semantics"
                        .into(),
                );
            }
            match value.mutation_mode {
                WorkflowVariableMutationMode::Immutable
                    if value.source_step_id.as_deref() == value.region_id.as_deref()
                        && !value.source_path.is_empty() =>
                {
                    require_source_schema(value)?;
                }
                WorkflowVariableMutationMode::Deterministic
                    if value.source_step_id.is_none() && value.source_schema_digest.is_none() => {}
                _ => {
                    return Err(
                        "Composite-local variables are region inputs or deterministic locals"
                            .into(),
                    );
                }
            }
            if value.storage_class != WorkflowVariableStorageClass::Inline {
                return Err("Composite-local frames cannot own opaque external material".into());
            }
        }
        WorkflowVariableScope::Run => {
            require_origin(
                value,
                None,
                None,
                WorkflowVariableMutationMode::Deterministic,
            )?;
            if value.source_schema_digest.is_some() {
                return Err("Run variables cannot declare an external source schema".into());
            }
            if !value.source_path.is_empty() {
                return Err("Run variables are initialized by defaults or assignments".into());
            }
            if value.storage_class != WorkflowVariableStorageClass::Inline {
                return Err(
                    "Run variables store bounded semantic values, not opaque material".into(),
                );
            }
        }
        WorkflowVariableScope::Application => {
            require_origin(
                value,
                None,
                None,
                WorkflowVariableMutationMode::OptimisticApplicationPort,
            )?;
            if value.source_schema_digest.is_some() {
                return Err(
                    "Application variables retain their owner-defined schema source".into(),
                );
            }
            if value.storage_class != WorkflowVariableStorageClass::Inline {
                return Err(
                    "Application variables are accessed only through their owner port".into(),
                );
            }
            if !value.source_path.is_empty() || value.default_value_digest.is_some() {
                return Err(
                    "Application variable initialization remains owned by Applications".into(),
                );
            }
        }
    }
    Ok(())
}

fn require_source_schema(value: &WorkflowVariableDeclaration) -> Result<(), String> {
    let source = value.source_schema_digest.as_ref().ok_or_else(|| {
        "Source-backed Workflow variables require a root schema digest".to_owned()
    })?;
    if value.source_path.is_empty() && source != &value.value_schema_digest {
        return Err("Whole-source Workflow variables must preserve the root schema digest".into());
    }
    Ok(())
}

fn require_origin(
    value: &WorkflowVariableDeclaration,
    step: Option<&str>,
    region: Option<&str>,
    mutation: WorkflowVariableMutationMode,
) -> Result<(), String> {
    if value.source_step_id.as_deref() != step
        || value.region_id.as_deref() != region
        || value.mutation_mode != mutation
    {
        return Err("Workflow variable scope origin or mutation mode is invalid".into());
    }
    Ok(())
}

fn validate_read(
    value: &WorkflowVariableRead,
    declarations: &BTreeMap<&str, &WorkflowVariableDeclaration>,
) -> Result<(), String> {
    validate_identifier("Workflow variable read ID", &value.id)?;
    validate_identifier("Workflow variable read name", &value.variable)?;
    validate_identifier("Workflow variable consumer step", &value.consumer_step_id)?;
    validate_identifier("Workflow variable target port", &value.target_port)?;
    validate_path("Workflow variable read path", &value.path)?;
    if let Some(region_id) = &value.consumer_region_id {
        validate_identifier("Workflow variable consumer region", region_id)?;
    }
    let declaration = declarations.get(value.variable.as_str()).ok_or_else(|| {
        format!(
            "Workflow variable read {:?} references an unknown declaration",
            value.id
        )
    })?;
    if value.path.is_empty()
        && (value.expected_type != declaration.value_type
            || value.expected_schema_digest != declaration.value_schema_digest)
    {
        return Err("Whole-variable reads must preserve the declared type and schema".into());
    }
    if value.required && !declaration.required && declaration.default_value_digest.is_none() {
        return Err("A required read cannot consume an optional variable without a default".into());
    }
    let expected_mode = match declaration.scope {
        WorkflowVariableScope::Application => WorkflowVariableReadMode::ApplicationPort,
        _ if declaration.storage_class != WorkflowVariableStorageClass::Inline => {
            WorkflowVariableReadMode::OpaqueReference
        }
        _ => WorkflowVariableReadMode::DirectValue,
    };
    if value.mode != expected_mode {
        return Err("Workflow variable read mode crosses its owning storage boundary".into());
    }
    if value.mode == WorkflowVariableReadMode::OpaqueReference && !value.path.is_empty() {
        return Err("Opaque Workflow references cannot be dereferenced by the compiler".into());
    }
    if declaration.scope == WorkflowVariableScope::CompositeLocal
        && value.consumer_region_id.as_deref() != declaration.region_id.as_deref()
    {
        return Err("Composite-local variables cannot escape their declared region".into());
    }
    Ok(())
}

fn validate_assignment(
    value: &WorkflowVariableAssignment,
    declarations: &BTreeMap<&str, &WorkflowVariableDeclaration>,
) -> Result<(), String> {
    validate_identifier("Workflow variable assignment ID", &value.id)?;
    validate_identifier(
        "Workflow variable assignment target",
        &value.target_variable,
    )?;
    validate_identifier(
        "Workflow variable assignment source",
        &value.source_variable,
    )?;
    validate_identifier(
        "Workflow variable assignment writer step",
        &value.writer_step_id,
    )?;
    validate_path(
        "Workflow variable assignment source path",
        &value.source_path,
    )?;
    if let Some(region_id) = &value.writer_region_id {
        validate_identifier("Workflow variable assignment writer region", region_id)?;
    }
    let target = declarations
        .get(value.target_variable.as_str())
        .ok_or_else(|| format!("unknown assignment target {:?}", value.target_variable))?;
    let source = declarations
        .get(value.source_variable.as_str())
        .ok_or_else(|| format!("unknown assignment source {:?}", value.source_variable))?;
    if target.mutation_mode == WorkflowVariableMutationMode::Immutable
        || target.value_type != value.value_type
        || target.value_schema_digest != value.value_schema_digest
    {
        return Err("Workflow assignment does not match a mutable target declaration".into());
    }
    if value.source_path.is_empty()
        && (source.value_type != value.value_type
            || source.value_schema_digest != value.value_schema_digest)
    {
        return Err("Whole-variable assignment must preserve source type and schema".into());
    }
    if source.storage_class != WorkflowVariableStorageClass::Inline {
        return Err("Opaque references cannot be copied into mutable Workflow values".into());
    }
    if target.scope == WorkflowVariableScope::CompositeLocal
        && value.writer_region_id.as_deref() != target.region_id.as_deref()
    {
        return Err("Composite-local assignment must remain inside its region".into());
    }
    if source.scope == WorkflowVariableScope::CompositeLocal
        && (value.writer_region_id.as_deref() != source.region_id.as_deref()
            || target.scope != WorkflowVariableScope::CompositeLocal
            || target.region_id != source.region_id)
    {
        return Err("Composite-local values leave their region only through an export".into());
    }
    let has_optimistic_evidence =
        value.expected_revision_variable.is_some() && value.idempotency_key_variable.is_some();
    match target.mutation_mode {
        WorkflowVariableMutationMode::OptimisticApplicationPort if !has_optimistic_evidence => {
            return Err(
                "Application assignment requires revision and idempotency variables".into(),
            );
        }
        WorkflowVariableMutationMode::OptimisticApplicationPort => {
            validate_evidence_variable(
                value.expected_revision_variable.as_deref(),
                WorkflowDataType::Number,
                declarations,
            )?;
            validate_evidence_variable(
                value.idempotency_key_variable.as_deref(),
                WorkflowDataType::String,
                declarations,
            )?;
        }
        _ if has_optimistic_evidence
            || value.expected_revision_variable.is_some()
            || value.idempotency_key_variable.is_some() =>
        {
            return Err("Only Application assignments use optimistic evidence variables".into());
        }
        _ => {}
    }
    Ok(())
}

fn validate_evidence_variable(
    name: Option<&str>,
    expected_type: WorkflowDataType,
    declarations: &BTreeMap<&str, &WorkflowVariableDeclaration>,
) -> Result<(), String> {
    let name = name.ok_or_else(|| "optimistic evidence variable is missing".to_owned())?;
    let declaration = declarations
        .get(name)
        .ok_or_else(|| format!("optimistic evidence variable {name:?} is not declared"))?;
    if declaration.value_type != expected_type
        || declaration.storage_class != WorkflowVariableStorageClass::Inline
    {
        return Err("optimistic evidence variable has the wrong type".into());
    }
    Ok(())
}

fn validate_export(
    value: &WorkflowVariableExport,
    declarations: &BTreeMap<&str, &WorkflowVariableDeclaration>,
) -> Result<(), String> {
    validate_identifier("Workflow variable export ID", &value.id)?;
    validate_identifier("Workflow variable export region", &value.region_id)?;
    validate_identifier("Workflow variable export source", &value.source_variable)?;
    validate_identifier("Workflow variable export target", &value.target_variable)?;
    validate_path("Workflow variable export source path", &value.source_path)?;
    let source = declarations
        .get(value.source_variable.as_str())
        .ok_or_else(|| format!("unknown export source {:?}", value.source_variable))?;
    let target = declarations
        .get(value.target_variable.as_str())
        .ok_or_else(|| format!("unknown export target {:?}", value.target_variable))?;
    if source.scope != WorkflowVariableScope::CompositeLocal
        || source.region_id.as_deref() != Some(value.region_id.as_str())
        || target.scope != WorkflowVariableScope::NodeOutput
        || target.source_step_id.as_deref() != Some(value.region_id.as_str())
        || !target.source_path.is_empty()
        || target.value_type != value.value_type
        || target.value_schema_digest != value.value_schema_digest
    {
        return Err("Workflow variable export does not cross one exact composite boundary".into());
    }
    if value.source_path.is_empty()
        && (source.value_type != value.value_type
            || source.value_schema_digest != value.value_schema_digest)
    {
        return Err("Whole-variable export must preserve source type and schema".into());
    }
    Ok(())
}

fn validate_mutation_order(values: &[WorkflowVariableAssignment]) -> Result<(), String> {
    let mut previous_target = None;
    let mut expected = 1u32;
    for value in values {
        if previous_target != Some(value.target_variable.as_str()) {
            previous_target = Some(value.target_variable.as_str());
            expected = 1;
        }
        if value.mutation_order != expected {
            return Err(
                "Workflow variable mutation order must be contiguous for each target".into(),
            );
        }
        expected = expected
            .checked_add(1)
            .ok_or_else(|| "Workflow variable mutation order overflowed".to_owned())?;
    }
    Ok(())
}

fn reject_duplicate_read_targets(values: &[WorkflowVariableRead]) -> Result<(), String> {
    let mut targets = BTreeSet::new();
    for value in values {
        if !targets.insert((
            value.consumer_step_id.as_str(),
            value.consumer_region_id.as_deref(),
            value.target_port.as_str(),
        )) {
            return Err("Workflow variable reads cannot bind one target port twice".into());
        }
    }
    Ok(())
}

fn validate_path(label: &str, path: &[String]) -> Result<(), String> {
    if path.len() > MAX_PATH_SEGMENTS {
        return Err(format!("{label} contains too many segments"));
    }
    for segment in path {
        validate_identifier(label, segment)?;
    }
    Ok(())
}

fn reject_duplicate_keys<'a>(
    values: impl Iterator<Item = &'a str>,
    label: &str,
) -> Result<(), String> {
    let values = values.collect::<Vec<_>>();
    let unique = values.iter().copied().collect::<BTreeSet<_>>();
    if unique.len() != values.len() {
        return Err(format!(
            "Workflow variable contract contains a duplicate {label}"
        ));
    }
    Ok(())
}
