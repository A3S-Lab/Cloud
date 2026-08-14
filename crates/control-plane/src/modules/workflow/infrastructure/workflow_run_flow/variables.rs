use crate::modules::shared_kernel::domain::canonical_json_bounded;
use crate::modules::workflow::domain::{
    lookup_workflow_variable_path, materialize_workflow_variables, WorkflowRunInput,
    WorkflowVariableReadMode, WORKFLOW_RUN_INPUT_MAX_BYTES,
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(super) struct WorkflowStepVariableProjection {
    pub input: Value,
    pub authoritative: bool,
}

pub(super) fn effective_input(
    input: &WorkflowRunInput,
    step_id: &str,
    legacy_input: Value,
    outputs: &BTreeMap<String, Value>,
) -> Result<WorkflowStepVariableProjection, String> {
    let Some(contract) = input.variable_contract.as_ref() else {
        return Ok(WorkflowStepVariableProjection {
            input: legacy_input,
            authoritative: false,
        });
    };
    let contract = contract.restore()?;
    let reads = contract
        .spec()
        .reads
        .iter()
        .filter(|read| read.consumer_step_id == step_id)
        .collect::<Vec<_>>();
    if reads.is_empty() {
        return Ok(WorkflowStepVariableProjection {
            input: legacy_input,
            authoritative: false,
        });
    }

    let values = materialize_workflow_variables(input, &contract, outputs)?;
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
    let projected = Value::Object(ports);
    canonical_json_bounded(
        &projected,
        WORKFLOW_RUN_INPUT_MAX_BYTES,
        "Workflow variable-projected step input",
    )?;
    Ok(WorkflowStepVariableProjection {
        input: projected,
        authoritative: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::Sha256Digest;
    use crate::modules::workflow::domain::{
        ResolvedWorkflowVariableContract, WorkflowDataType, WorkflowVariableAssignment,
        WorkflowVariableContract, WorkflowVariableContractSpec, WorkflowVariableDeclaration,
        WorkflowVariableMutationMode, WorkflowVariableRead, WorkflowVariableScope,
        WorkflowVariableStorageClass, WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
    };

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    #[test]
    fn projection_materializes_invocation_node_output_and_run_assignments() {
        let mut input = crate::modules::workflow::test_support::workflow_run_input()
            .expect("WorkflowRun input");
        let contract = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
            id: "support.runtime".into(),
            revision: "1.0.0".into(),
            compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
            declarations: vec![
                declaration(
                    "request",
                    WorkflowVariableScope::InvocationInput,
                    WorkflowDataType::Object,
                    Some(digest('a')),
                    None,
                    vec![],
                    WorkflowVariableMutationMode::Immutable,
                ),
                declaration(
                    "triage_result",
                    WorkflowVariableScope::NodeOutput,
                    WorkflowDataType::Object,
                    Some(digest('b')),
                    Some("normalize"),
                    vec![],
                    WorkflowVariableMutationMode::Immutable,
                ),
                declaration(
                    "summary",
                    WorkflowVariableScope::Run,
                    WorkflowDataType::String,
                    None,
                    None,
                    vec![],
                    WorkflowVariableMutationMode::Deterministic,
                ),
            ],
            reads: vec![WorkflowVariableRead {
                id: "output-summary".into(),
                variable: "summary".into(),
                consumer_step_id: "output".into(),
                consumer_region_id: None,
                target_port: "result".into(),
                path: vec![],
                expected_type: WorkflowDataType::String,
                expected_schema_digest: digest('c'),
                required: true,
                mode: WorkflowVariableReadMode::DirectValue,
            }],
            assignments: vec![WorkflowVariableAssignment {
                id: "assign-summary".into(),
                target_variable: "summary".into(),
                source_variable: "triage_result".into(),
                writer_step_id: "normalize".into(),
                writer_region_id: None,
                source_path: vec!["summary".into()],
                value_type: WorkflowDataType::String,
                value_schema_digest: digest('c'),
                mutation_order: 1,
                expected_revision_variable: None,
                idempotency_key_variable: None,
            }],
            exports: vec![],
        })
        .expect("variable contract");
        input.variable_contract = Some(ResolvedWorkflowVariableContract::from_contract(&contract));

        let projected = effective_input(
            &input,
            "output",
            Value::Null,
            &BTreeMap::from([(
                "normalize".into(),
                serde_json::json!({"summary": "HIGH T-42"}),
            )]),
        )
        .expect("projected input");
        assert!(projected.authoritative);
        assert_eq!(projected.input, serde_json::json!({"result": "HIGH T-42"}));
    }

    #[test]
    fn one_writer_resolves_all_assignment_sources_from_its_pre_write_snapshot() {
        let input = crate::modules::workflow::test_support::workflow_run_input()
            .expect("WorkflowRun input");
        let contract = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
            id: "support.atomic-writer".into(),
            revision: "1.0.0".into(),
            compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
            declarations: vec![
                declaration(
                    "request",
                    WorkflowVariableScope::InvocationInput,
                    WorkflowDataType::Object,
                    Some(digest('a')),
                    None,
                    vec![],
                    WorkflowVariableMutationMode::Immutable,
                ),
                declaration(
                    "current_value",
                    WorkflowVariableScope::Run,
                    WorkflowDataType::String,
                    None,
                    None,
                    vec![],
                    WorkflowVariableMutationMode::Deterministic,
                ),
                declaration(
                    "copied_value",
                    WorkflowVariableScope::Run,
                    WorkflowDataType::String,
                    None,
                    None,
                    vec![],
                    WorkflowVariableMutationMode::Deterministic,
                ),
            ],
            reads: vec![],
            assignments: vec![
                assignment(
                    "initialize-current",
                    "current_value",
                    "request",
                    "normalize",
                    vec!["ticketId".into()],
                    1,
                ),
                assignment(
                    "replace-current",
                    "current_value",
                    "request",
                    "high",
                    vec!["priority".into()],
                    2,
                ),
                assignment(
                    "copy-current",
                    "copied_value",
                    "current_value",
                    "high",
                    vec![],
                    1,
                ),
            ],
            exports: vec![],
        })
        .expect("atomic-writer contract");
        let values = materialize_workflow_variables(
            &input,
            &contract,
            &BTreeMap::from([
                ("normalize".into(), serde_json::json!({})),
                ("high".into(), serde_json::json!({})),
            ]),
        )
        .expect("materialized assignments");

        assert_eq!(
            values.get("current_value"),
            Some(&serde_json::json!("high"))
        );
        assert_eq!(values.get("copied_value"), Some(&serde_json::json!("T-42")));
    }

    fn assignment(
        id: &str,
        target_variable: &str,
        source_variable: &str,
        writer_step_id: &str,
        source_path: Vec<String>,
        mutation_order: u32,
    ) -> WorkflowVariableAssignment {
        WorkflowVariableAssignment {
            id: id.into(),
            target_variable: target_variable.into(),
            source_variable: source_variable.into(),
            writer_step_id: writer_step_id.into(),
            writer_region_id: None,
            source_path,
            value_type: WorkflowDataType::String,
            value_schema_digest: digest('c'),
            mutation_order,
            expected_revision_variable: None,
            idempotency_key_variable: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn declaration(
        name: &str,
        scope: WorkflowVariableScope,
        value_type: WorkflowDataType,
        source_schema_digest: Option<Sha256Digest>,
        source_step_id: Option<&str>,
        source_path: Vec<String>,
        mutation_mode: WorkflowVariableMutationMode,
    ) -> WorkflowVariableDeclaration {
        WorkflowVariableDeclaration {
            name: name.into(),
            scope,
            value_type,
            value_schema_digest: match name {
                "request" => digest('a'),
                "triage_result" => digest('b'),
                _ => digest('c'),
            },
            source_schema_digest,
            storage_class: WorkflowVariableStorageClass::Inline,
            mutation_mode,
            required: true,
            source_step_id: source_step_id.map(str::to_owned),
            source_path,
            region_id: None,
            default_value_digest: None,
        }
    }
}
