use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, PlanRevisionId, Sha256Digest,
    WorkflowRunId,
};
use crate::modules::workflow::domain::{
    WorkflowDataType, WorkflowRunInput, WorkflowRunRecord, WorkflowVariableAssignment,
    WorkflowVariableContract, WorkflowVariableDeclaration, WorkflowVariableMutationMode,
    WorkflowVariableScope, WorkflowVariableStorageClass, WORKFLOW_RUN_OUTPUT_MAX_BYTES,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const WORKFLOW_RUN_VARIABLE_INSPECTION_SCHEMA: &str =
    "cloud.workflow-run.variable-inspection.v1";
pub const WORKFLOW_RUN_VARIABLE_INSPECTION_MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunVariableState {
    Materialized,
    Unavailable,
}

impl WorkflowRunVariableState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Materialized => "materialized",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunVariable {
    pub name: String,
    pub scope: WorkflowVariableScope,
    pub value_type: WorkflowDataType,
    pub value_schema_digest: Sha256Digest,
    pub storage_class: WorkflowVariableStorageClass,
    pub mutation_mode: WorkflowVariableMutationMode,
    pub required: bool,
    pub source_step_id: Option<String>,
    pub state: WorkflowRunVariableState,
    pub redacted: bool,
    pub value: Option<Value>,
    pub value_digest: Option<Sha256Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunVariableInspection {
    pub schema: String,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub variable_contract_digest: Sha256Digest,
    pub last_flow_sequence: u64,
    pub observed_at: DateTime<Utc>,
    pub variables: Vec<WorkflowRunVariable>,
}

#[async_trait]
pub trait IWorkflowRunVariableReader: Send + Sync {
    async fn inspect(
        &self,
        record: &WorkflowRunRecord,
    ) -> Result<WorkflowRunVariableInspection, String>;
}

pub fn inspect_workflow_run_variables(
    record: &WorkflowRunRecord,
    last_flow_sequence: u64,
    observed_at: DateTime<Utc>,
    outputs: &BTreeMap<String, Value>,
) -> Result<WorkflowRunVariableInspection, String> {
    record.validate()?;
    if last_flow_sequence < record.run.last_flow_sequence {
        return Err("Workflow variable inspection precedes the persisted Flow projection".into());
    }
    let observed_at = canonical_timestamp(observed_at);
    if observed_at < record.run.requested_at {
        return Err("Workflow variable inspection time precedes the run request".into());
    }
    let resolved = record
        .run
        .execution_input
        .variable_contract
        .as_ref()
        .ok_or_else(|| "WorkflowRun does not carry an exact typed variable contract".to_owned())?;
    let contract = resolved.restore()?;
    let values = materialize_workflow_variables(&record.run.execution_input, &contract, outputs)?;
    let variables = contract
        .spec()
        .declarations
        .iter()
        .map(|declaration| variable_projection(declaration, values.get(&declaration.name)))
        .collect::<Result<Vec<_>, _>>()?;
    let inspection = WorkflowRunVariableInspection {
        schema: WORKFLOW_RUN_VARIABLE_INSPECTION_SCHEMA.into(),
        workflow_run_id: record.run.id,
        plan_revision_id: record.run.plan_revision_id,
        variable_contract_digest: resolved.digest.clone(),
        last_flow_sequence,
        observed_at,
        variables,
    };
    canonical_json_bounded(
        &inspection,
        WORKFLOW_RUN_VARIABLE_INSPECTION_MAX_BYTES,
        "Workflow variable inspection",
    )?;
    Ok(inspection)
}

fn variable_projection(
    declaration: &WorkflowVariableDeclaration,
    value: Option<&Value>,
) -> Result<WorkflowRunVariable, String> {
    let value_digest = value
        .map(|value| {
            let canonical = canonical_json_bounded(
                value,
                WORKFLOW_RUN_OUTPUT_MAX_BYTES,
                "Workflow variable value",
            )?;
            Sha256Digest::parse(sha256_digest(&canonical))
        })
        .transpose()?;
    let redacted = value.is_some()
        && declaration.storage_class == WorkflowVariableStorageClass::SecretReference;
    Ok(WorkflowRunVariable {
        name: declaration.name.clone(),
        scope: declaration.scope,
        value_type: declaration.value_type.clone(),
        value_schema_digest: declaration.value_schema_digest.clone(),
        storage_class: declaration.storage_class,
        mutation_mode: declaration.mutation_mode,
        required: declaration.required,
        source_step_id: declaration.source_step_id.clone(),
        state: if value.is_some() {
            WorkflowRunVariableState::Materialized
        } else {
            WorkflowRunVariableState::Unavailable
        },
        redacted,
        value: if redacted { None } else { value.cloned() },
        value_digest,
    })
}

pub(crate) fn materialize_workflow_variables(
    input: &WorkflowRunInput,
    contract: &WorkflowVariableContract,
    outputs: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>, String> {
    let mut values = BTreeMap::new();
    for declaration in &contract.spec().declarations {
        let source = match declaration.scope {
            WorkflowVariableScope::InvocationInput => Some(&input.goal_input),
            WorkflowVariableScope::NodeOutput => declaration
                .source_step_id
                .as_ref()
                .and_then(|step_id| outputs.get(step_id)),
            WorkflowVariableScope::Run => None,
            WorkflowVariableScope::CompositeLocal | WorkflowVariableScope::Application => {
                return Err(format!(
                    "WorkflowRun runtime v2 cannot materialize {} variable {:?}",
                    declaration.scope.as_str(),
                    declaration.name
                ));
            }
        };
        if let Some(value) = materialize_declaration(declaration, source)? {
            values.insert(declaration.name.clone(), value);
        }
    }

    for step in &input.plan.steps {
        if !outputs.contains_key(&step.id) {
            continue;
        }
        let updates = contract
            .spec()
            .assignments
            .iter()
            .filter(|assignment| assignment.writer_step_id == step.id)
            .map(|assignment| {
                resolve_assignment(assignment, &values)
                    .map(|value| (assignment.target_variable.clone(), value))
            })
            .collect::<Result<Vec<_>, String>>()?;
        for (target, value) in updates {
            values.insert(target, value);
        }
    }
    Ok(values)
}

fn materialize_declaration(
    declaration: &WorkflowVariableDeclaration,
    source: Option<&Value>,
) -> Result<Option<Value>, String> {
    let Some(source) = source else {
        if declaration.required && declaration.scope == WorkflowVariableScope::InvocationInput {
            return Err(format!(
                "required Workflow variable {:?} source is unavailable",
                declaration.name
            ));
        }
        return Ok(None);
    };
    let Some(value) = lookup_workflow_variable_path(source, &declaration.source_path) else {
        if declaration.required {
            return Err(format!(
                "required Workflow variable {:?} source path is unavailable",
                declaration.name
            ));
        }
        return Ok(None);
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

fn resolve_assignment(
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

    #[test]
    fn secret_reference_values_are_digest_visible_but_value_redacted() {
        let digest =
            Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("schema digest");
        let declaration = WorkflowVariableDeclaration {
            name: "credential".into(),
            scope: WorkflowVariableScope::InvocationInput,
            value_type: WorkflowDataType::Object,
            value_schema_digest: digest.clone(),
            source_schema_digest: Some(digest),
            storage_class: WorkflowVariableStorageClass::SecretReference,
            mutation_mode: WorkflowVariableMutationMode::Immutable,
            required: true,
            source_step_id: None,
            source_path: vec!["credential".into()],
            region_id: None,
            default_value_digest: None,
        };

        let variable = variable_projection(
            &declaration,
            Some(&serde_json::json!({"secretId": "secret-1", "revision": 3})),
        )
        .expect("secret projection");

        assert_eq!(variable.state, WorkflowRunVariableState::Materialized);
        assert!(variable.redacted);
        assert!(variable.value.is_none());
        assert!(variable.value_digest.is_some());
    }
}
