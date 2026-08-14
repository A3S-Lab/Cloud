use super::{
    inspect_workflow_run_variables, WorkflowRun, WorkflowRunRecord, WorkflowRunVariableState,
    WORKFLOW_RUN_VARIABLE_INSPECTION_SCHEMA,
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
