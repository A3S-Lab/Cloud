use super::{
    inspect_workflow_run_variables, ResolvedWorkflowVariableContract,
    ResolvedWorkflowVariableDefaults, WorkflowDataType, WorkflowRun, WorkflowRunRecord,
    WorkflowRunVariableState, WorkflowVariableContract, WorkflowVariableDeclaration,
    WorkflowVariableDefault, WorkflowVariableDefaults, WorkflowVariableDefaultsSpec,
    WorkflowVariableMutationMode, WorkflowVariableScope, WorkflowVariableStorageClass,
    WORKFLOW_PLAN_MAX_BYTES, WORKFLOW_RUN_VARIABLE_INSPECTION_SCHEMA,
};
use crate::modules::shared_kernel::domain::{canonical_json_bounded, sha256_digest, PrincipalId};
use crate::modules::workflow::domain::WORKFLOW_RUN_OUTPUT_MAX_BYTES;
use crate::modules::workflow::test_support::{
    timestamp, typed_variable_workflow_run_input, workflow_run_input,
};
use std::collections::BTreeMap;

#[test]
fn inspection_materializes_the_exact_typed_contract_without_a_variable_store() {
    let input = typed_variable_workflow_run_input().expect("typed WorkflowRun input");
    let expected_value = input.goal_input.clone();
    let contract_digest = input
        .variable_contract
        .as_ref()
        .expect("variable contract")
        .digest
        .clone();
    let (run, steps) = WorkflowRun::create(input, PrincipalId::new()).expect("WorkflowRun");
    let record = WorkflowRunRecord { run, steps };

    let inspection =
        inspect_workflow_run_variables(&record, 0, record.run.requested_at, &BTreeMap::new())
            .expect("variable inspection");

    assert_eq!(inspection.schema, WORKFLOW_RUN_VARIABLE_INSPECTION_SCHEMA);
    assert_eq!(inspection.workflow_run_id, record.run.id);
    assert_eq!(inspection.plan_revision_id, record.run.plan_revision_id);
    assert_eq!(inspection.variable_contract_digest, contract_digest);
    assert_eq!(inspection.last_flow_sequence, 0);
    assert_eq!(inspection.observed_at, timestamp(8, 0));
    assert_eq!(inspection.variables.len(), 1);
    let variable = &inspection.variables[0];
    assert_eq!(variable.name, "request");
    assert_eq!(variable.state, WorkflowRunVariableState::Materialized);
    assert_eq!(variable.value.as_ref(), Some(&expected_value));
    let canonical = canonical_json_bounded(
        &expected_value,
        WORKFLOW_RUN_OUTPUT_MAX_BYTES,
        "test Workflow variable",
    )
    .expect("canonical variable");
    assert_eq!(
        variable.value_digest.as_ref().map(ToString::to_string),
        Some(sha256_digest(&canonical))
    );
}

#[test]
fn inspection_fails_closed_without_an_exact_typed_variable_contract() {
    let input = workflow_run_input().expect("legacy WorkflowRun input");
    let (run, steps) = WorkflowRun::create(input, PrincipalId::new()).expect("WorkflowRun");
    let record = WorkflowRunRecord { run, steps };

    let error =
        inspect_workflow_run_variables(&record, 0, record.run.requested_at, &BTreeMap::new())
            .expect_err("legacy run must not invent variable authority");

    assert!(error.contains("typed variable contract"));
}

#[test]
fn inspection_reconstructs_revision_owned_defaults_from_immutable_run_input() {
    let mut input = typed_variable_workflow_run_input().expect("typed WorkflowRun input");
    let default =
        WorkflowVariableDefault::new("fallback", serde_json::json!("normal")).expect("default");
    let resolved = input
        .variable_contract
        .as_ref()
        .expect("variable contract")
        .restore()
        .expect("restored contract");
    let mut spec = resolved.spec().clone();
    spec.declarations.push(WorkflowVariableDeclaration {
        name: "fallback".into(),
        scope: WorkflowVariableScope::Run,
        value_type: WorkflowDataType::String,
        value_schema_digest: crate::modules::shared_kernel::domain::Sha256Digest::parse(format!(
            "sha256:{}",
            "c".repeat(64)
        ))
        .expect("schema digest"),
        source_schema_digest: None,
        storage_class: WorkflowVariableStorageClass::Inline,
        mutation_mode: WorkflowVariableMutationMode::Deterministic,
        required: false,
        source_step_id: None,
        source_path: Vec::new(),
        region_id: None,
        default_value_digest: Some(default.digest.clone()),
    });
    let contract = WorkflowVariableContract::from_spec(spec).expect("contract with default");
    let defaults = WorkflowVariableDefaults::from_spec(WorkflowVariableDefaultsSpec {
        id: contract.id().into(),
        revision: contract.revision().into(),
        values: vec![default],
    })
    .expect("defaults");
    input.plan.variable_contract_digest = Some(contract.digest().clone());
    input.variable_contract = Some(ResolvedWorkflowVariableContract::from_contract(&contract));
    input.variable_defaults = Some(ResolvedWorkflowVariableDefaults::from_defaults(&defaults));
    input.plan_digest = crate::modules::shared_kernel::domain::Sha256Digest::parse(sha256_digest(
        &canonical_json_bounded(&input.plan, WORKFLOW_PLAN_MAX_BYTES, "test plan")
            .expect("canonical plan"),
    ))
    .expect("plan digest");
    let (run, steps) = WorkflowRun::create(input, PrincipalId::new()).expect("WorkflowRun");
    let record = WorkflowRunRecord { run, steps };

    let inspection =
        inspect_workflow_run_variables(&record, 0, record.run.requested_at, &BTreeMap::new())
            .expect("variable inspection");
    let fallback = inspection
        .variables
        .iter()
        .find(|variable| variable.name == "fallback")
        .expect("fallback projection");
    assert_eq!(fallback.state, WorkflowRunVariableState::Materialized);
    assert_eq!(fallback.value, Some(serde_json::json!("normal")));
    assert_eq!(
        fallback.value_digest.as_ref(),
        defaults.spec().values.first().map(|value| &value.digest)
    );
}
