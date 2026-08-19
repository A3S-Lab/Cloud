use super::{
    WorkflowRunInput, WorkflowVariableAssignment, WorkflowVariableContract,
    WorkflowVariableDeclaration, WorkflowVariableReadMode, WorkflowVariableScope,
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Reconstructs parent-scope values from immutable Run input and Flow-observed
/// step outputs. Composite-local assignments are deliberately excluded: the
/// exact composite frame owns those mutations and exports.
pub(crate) fn materialize_workflow_variables(
    input: &WorkflowRunInput,
    contract: &WorkflowVariableContract,
    outputs: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>, String> {
    let defaults = input
        .variable_defaults
        .as_ref()
        .map(|resolved| resolved.restore())
        .transpose()?;
    let requires_defaults = contract
        .spec()
        .declarations
        .iter()
        .any(|declaration| declaration.default_value_digest.is_some());
    match (requires_defaults, defaults.as_ref()) {
        (false, None) => {}
        (true, Some(defaults)) => defaults.validate_contract(contract)?,
        (true, None) => {
            return Err("Workflow variable default material is unavailable".into());
        }
        (false, Some(_)) => {
            return Err("Workflow variable default material is unreferenced".into());
        }
    }
    let mut values = BTreeMap::new();
    for declaration in &contract.spec().declarations {
        let source = match declaration.scope {
            WorkflowVariableScope::InvocationInput => Some(&input.goal_input),
            WorkflowVariableScope::NodeOutput => declaration
                .source_step_id
                .as_ref()
                .and_then(|step_id| outputs.get(step_id)),
            WorkflowVariableScope::Run => None,
            WorkflowVariableScope::CompositeLocal => continue,
            WorkflowVariableScope::Application => {
                return Err(format!(
                    "WorkflowRun runtime v2 cannot materialize {} variable {:?}",
                    declaration.scope.as_str(),
                    declaration.name
                ));
            }
        };
        let default = defaults
            .as_ref()
            .and_then(|defaults| defaults.value(&declaration.name));
        if let Some(value) =
            materialize_workflow_variable_declaration(declaration, source, default)?
        {
            values.insert(declaration.name.clone(), value);
        }
    }

    for step in &input.plan.steps {
        if !outputs.contains_key(&step.id) {
            continue;
        }
        if step.kind == super::WorkflowStepKind::Subworkflow {
            continue;
        }
        let updates = contract
            .spec()
            .assignments
            .iter()
            .filter(|assignment| {
                assignment.writer_step_id == step.id && assignment.writer_region_id.is_none()
            })
            .map(|assignment| {
                resolve_workflow_variable_assignment(assignment, &values)
                    .map(|value| (assignment.target_variable.clone(), value))
            })
            .collect::<Result<Vec<_>, String>>()?;
        for (target, value) in updates {
            values.insert(target, value);
        }
    }
    Ok(values)
}

pub(crate) fn materialize_workflow_variable_declaration(
    declaration: &WorkflowVariableDeclaration,
    source: Option<&Value>,
    default: Option<&Value>,
) -> Result<Option<Value>, String> {
    let value = match source {
        Some(source) => {
            match lookup_workflow_variable_path(source, &declaration.source_path).or(default) {
                Some(value) => value,
                None if declaration.required => {
                    return Err(format!(
                        "required Workflow variable {:?} source path is unavailable",
                        declaration.name
                    ));
                }
                None => return Ok(None),
            }
        }
        None => match default {
            Some(value) => value,
            None if declaration.required
                && declaration.scope == WorkflowVariableScope::InvocationInput =>
            {
                return Err(format!(
                    "required Workflow variable {:?} source is unavailable",
                    declaration.name
                ));
            }
            None => return Ok(None),
        },
    };
    if !declaration.value_type.matches_json_value(value) {
        return Err(format!(
            "Workflow variable {:?} value does not match {}",
            declaration.name,
            declaration.value_type.as_str()
        ));
    }
    Ok(Some(value.clone()))
}

pub(crate) fn resolve_workflow_variable_assignment(
    assignment: &WorkflowVariableAssignment,
    values: &BTreeMap<String, Value>,
) -> Result<Value, String> {
    let source = values.get(&assignment.source_variable).ok_or_else(|| {
        format!(
            "Workflow assignment {:?} source {:?} is unavailable",
            assignment.id, assignment.source_variable
        )
    })?;
    let value =
        lookup_workflow_variable_path(source, &assignment.source_path).ok_or_else(|| {
            format!(
                "Workflow assignment {:?} source path is unavailable",
                assignment.id
            )
        })?;
    if !assignment.value_type.matches_json_value(value) {
        return Err(format!(
            "Workflow assignment {:?} value does not match {}",
            assignment.id,
            assignment.value_type.as_str()
        ));
    }
    Ok(value.clone())
}

pub(crate) fn project_workflow_variable_reads(
    contract: &WorkflowVariableContract,
    step_id: &str,
    values: &BTreeMap<String, Value>,
) -> Result<Option<Value>, String> {
    let reads = contract
        .spec()
        .reads
        .iter()
        .filter(|read| read.consumer_step_id == step_id)
        .collect::<Vec<_>>();
    if reads.is_empty() {
        return Ok(None);
    }

    let mut ports = Map::new();
    for read in reads {
        let Some(variable) = values.get(&read.variable) else {
            if read.required {
                return Err(format!(
                    "required Workflow variable read {:?} is unavailable",
                    read.id
                ));
            }
            continue;
        };
        let value = if read.mode == WorkflowVariableReadMode::OpaqueReference {
            variable
        } else if let Some(value) = lookup_workflow_variable_path(variable, &read.path) {
            value
        } else if read.required {
            return Err(format!(
                "Workflow variable read {:?} path is unavailable",
                read.id
            ));
        } else {
            continue;
        };
        if !read.expected_type.matches_json_value(value) {
            return Err(format!(
                "Workflow variable read {:?} value does not match {}",
                read.id,
                read.expected_type.as_str()
            ));
        }
        ports.insert(read.target_port.clone(), value.clone());
    }
    Ok(Some(Value::Object(ports)))
}

pub(crate) fn lookup_workflow_variable_path<'a>(
    mut value: &'a Value,
    path: &[String],
) -> Option<&'a Value> {
    for segment in path {
        value = match value {
            Value::Object(object) => object.get(segment)?,
            Value::Array(values) => values.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::Sha256Digest;
    use crate::modules::workflow::domain::{
        WorkflowDataType, WorkflowVariableContractSpec, WorkflowVariableMutationMode,
        WorkflowVariableStorageClass, WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
    };

    fn declaration(required: bool) -> WorkflowVariableDeclaration {
        let digest =
            Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("schema digest");
        WorkflowVariableDeclaration {
            name: "result".into(),
            scope: WorkflowVariableScope::NodeOutput,
            value_type: WorkflowDataType::String,
            value_schema_digest: digest.clone(),
            source_schema_digest: Some(digest),
            storage_class: WorkflowVariableStorageClass::Inline,
            mutation_mode: WorkflowVariableMutationMode::Immutable,
            required,
            source_step_id: Some("source".into()),
            source_path: vec!["value".into()],
            region_id: None,
            default_value_digest: None,
        }
    }

    #[test]
    fn missing_required_source_path_remains_an_error() {
        let error = materialize_workflow_variable_declaration(
            &declaration(true),
            Some(&serde_json::json!({})),
            None,
        )
        .expect_err("missing required source path");

        assert!(error.contains("source path is unavailable"));
    }

    #[test]
    fn optional_declaration_uses_default_when_its_source_path_is_absent() {
        assert_eq!(
            materialize_workflow_variable_declaration(
                &declaration(false),
                Some(&serde_json::json!({})),
                Some(&serde_json::json!("fallback")),
            )
            .expect("default materialization"),
            Some(serde_json::json!("fallback"))
        );
    }

    #[test]
    fn projection_returns_none_only_when_the_step_has_no_explicit_reads() {
        let contract = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
            id: "support.projection".into(),
            revision: "1.0.0".into(),
            compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
            declarations: vec![declaration(false)],
            reads: Vec::new(),
            assignments: Vec::new(),
            exports: Vec::new(),
        })
        .expect("contract");

        assert_eq!(
            project_workflow_variable_reads(&contract, "consumer", &BTreeMap::new())
                .expect("projection"),
            None
        );
    }

    #[test]
    fn parent_materialization_leaves_subworkflow_assignments_to_the_composite_frame() {
        let mut input = crate::modules::workflow::test_support::workflow_run_input()
            .expect("WorkflowRun input");
        input
            .plan
            .steps
            .iter_mut()
            .find(|step| step.id == "normalize")
            .expect("normalize step")
            .kind = super::super::WorkflowStepKind::Subworkflow;

        let mut source = declaration(true);
        source.name = "child_result".into();
        source.source_step_id = Some("normalize".into());
        source.source_path = vec!["summary".into()];
        let mut target = declaration(false);
        target.name = "summary".into();
        target.scope = WorkflowVariableScope::Run;
        target.source_schema_digest = None;
        target.source_step_id = None;
        target.source_path.clear();
        target.mutation_mode = WorkflowVariableMutationMode::Deterministic;
        let contract = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
            id: "support.composite-frame-ownership".into(),
            revision: "1.0.0".into(),
            compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
            declarations: vec![source, target],
            reads: Vec::new(),
            assignments: vec![WorkflowVariableAssignment {
                id: "update-summary".into(),
                target_variable: "summary".into(),
                source_variable: "child_result".into(),
                writer_step_id: "normalize".into(),
                writer_region_id: None,
                source_path: Vec::new(),
                value_type: WorkflowDataType::String,
                value_schema_digest: declaration(false).value_schema_digest,
                mutation_order: 1,
                expected_revision_variable: None,
                idempotency_key_variable: None,
            }],
            exports: Vec::new(),
        })
        .expect("variable contract");

        let values = materialize_workflow_variables(
            &input,
            &contract,
            &BTreeMap::from([(
                "normalize".into(),
                serde_json::json!({"summary": "frame-owned"}),
            )]),
        )
        .expect("parent materialization");

        assert_eq!(
            values.get("child_result"),
            Some(&serde_json::json!("frame-owned"))
        );
        assert!(!values.contains_key("summary"));
    }
}
