use super::*;
use crate::modules::shared_kernel::domain::{canonical_json_bounded, sha256_digest, Sha256Digest};
use crate::modules::workflow::test_support::{
    human_decision_workflow_run_input, typed_variable_workflow_run_input, workflow_run_input,
    TEST_HUMAN_STEP_ID,
};

#[test]
fn immutable_run_input_rejects_plan_input_payload_and_branch_drift() {
    let input = workflow_run_input().expect("valid WorkflowRun input");
    input.validate().expect("valid input");

    let mut goal_drift = input.clone();
    goal_drift.goal_input["priority"] = serde_json::json!("normal");
    assert!(goal_drift.validate().is_err());

    let mut payload_order_drift = input.clone();
    payload_order_drift.payloads.swap(0, 1);
    assert!(payload_order_drift.validate().is_err());

    let mut branch_drift = input;
    branch_drift
        .plan
        .edges
        .iter_mut()
        .find(|edge| edge.id == "route-high")
        .expect("high branch edge")
        .source_handle = Some("unexpected".into());
    refresh_plan_digest(&mut branch_drift);
    assert!(branch_drift.validate().is_err());
}

#[test]
fn v1_run_input_remains_byte_stable_without_v2_contract_fields() {
    let input = workflow_run_input().expect("valid WorkflowRun input");
    let encoded =
        String::from_utf8(input.canonical_bytes().expect("canonical v1 input")).expect("UTF-8");
    assert!(!encoded.contains("variable_contract"));
    assert!(!encoded.contains("composite_regions"));
    assert!(encoded.contains("\"schema\":\"cloud.workflow-run.input.v1\""));
    assert!(encoded.contains("\"flow_workflow_version\":\"1\""));
}

#[test]
fn v2_run_input_rejects_version_and_variable_contract_drift() {
    let input = typed_variable_workflow_run_input().expect("valid v2 WorkflowRun input");
    input.validate().expect("valid v2 input");
    let encoded =
        String::from_utf8(input.canonical_bytes().expect("canonical v2 input")).expect("UTF-8");
    assert!(!encoded.contains("composite_regions"));
    assert!(!encoded.contains("composite_regions_digest"));

    let mut version_drift = input.clone();
    version_drift.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION.into();
    assert!(version_drift.validate().is_err());

    let mut contract_drift = input;
    contract_drift
        .variable_contract
        .as_mut()
        .expect("variable contract")
        .digest = test_digest('f');
    assert!(contract_drift.validate().is_err());
}

#[test]
fn runtime_v2_requires_exact_default_material_and_rejects_external_variable_ownership() {
    let input = typed_variable_workflow_run_input().expect("valid v2 WorkflowRun input");
    let base = input
        .variable_contract
        .as_ref()
        .expect("variable contract")
        .restore()
        .expect("restored variable contract")
        .spec()
        .clone();

    let default = WorkflowVariableDefault::new("fallback", serde_json::json!("normal"))
        .expect("default material");
    let mut default_spec = base.clone();
    default_spec.declarations.push(WorkflowVariableDeclaration {
        name: "fallback".into(),
        scope: WorkflowVariableScope::Run,
        value_type: WorkflowDataType::String,
        value_schema_digest: test_digest('a'),
        source_schema_digest: None,
        storage_class: WorkflowVariableStorageClass::Inline,
        mutation_mode: WorkflowVariableMutationMode::Deterministic,
        required: false,
        source_step_id: None,
        source_path: Vec::new(),
        region_id: None,
        default_value_digest: Some(default.digest.clone()),
    });
    let default_contract = WorkflowVariableContract::from_spec(default_spec)
        .expect("valid digest-backed default contract");
    let default_error = validate_runtime_variable_contract(&default_contract, None, &input.plan)
        .expect_err("runtime must reject a digest without materialized default bytes");
    assert!(default_error.contains("digest-only default"));
    let defaults = WorkflowVariableDefaults::from_spec(WorkflowVariableDefaultsSpec {
        id: default_contract.id().into(),
        revision: default_contract.revision().into(),
        values: vec![default],
    })
    .expect("default set");
    validate_runtime_variable_contract(&default_contract, Some(&defaults), &input.plan)
        .expect("digest-backed default material");

    let mut composite_spec = base.clone();
    composite_spec
        .declarations
        .push(WorkflowVariableDeclaration {
            name: "local_value".into(),
            scope: WorkflowVariableScope::CompositeLocal,
            value_type: WorkflowDataType::String,
            value_schema_digest: test_digest('c'),
            source_schema_digest: None,
            storage_class: WorkflowVariableStorageClass::Inline,
            mutation_mode: WorkflowVariableMutationMode::Deterministic,
            required: false,
            source_step_id: None,
            source_path: Vec::new(),
            region_id: Some("normalize".into()),
            default_value_digest: None,
        });
    let composite_contract = WorkflowVariableContract::from_spec(composite_spec)
        .expect("valid composite-local contract");
    let composite_error =
        validate_runtime_variable_contract(&composite_contract, None, &input.plan)
            .expect_err("runtime must reject composite-local state");
    assert!(composite_error.contains("composite_local"));

    let mut application_spec = base;
    application_spec
        .declarations
        .push(WorkflowVariableDeclaration {
            name: "conversation".into(),
            scope: WorkflowVariableScope::Application,
            value_type: WorkflowDataType::String,
            value_schema_digest: test_digest('d'),
            source_schema_digest: None,
            storage_class: WorkflowVariableStorageClass::Inline,
            mutation_mode: WorkflowVariableMutationMode::OptimisticApplicationPort,
            required: false,
            source_step_id: None,
            source_path: Vec::new(),
            region_id: None,
            default_value_digest: None,
        });
    let application_contract = WorkflowVariableContract::from_spec(application_spec)
        .expect("valid application-owned contract");
    let application_error =
        validate_runtime_variable_contract(&application_contract, None, &input.plan)
            .expect_err("runtime must reject application-owned state");
    assert!(application_error.contains("application"));
}

#[test]
fn runtime_v2_rejects_reads_for_steps_without_projected_input_support() {
    let input = typed_variable_workflow_run_input().expect("valid v2 WorkflowRun input");
    let contract = input
        .variable_contract
        .as_ref()
        .expect("variable contract")
        .restore()
        .expect("restored variable contract");
    let mut spec = contract.spec().clone();
    let request = spec
        .declarations
        .iter()
        .find(|declaration| declaration.name == "request")
        .expect("request declaration")
        .clone();
    spec.reads.push(WorkflowVariableRead {
        id: "input-request".into(),
        variable: request.name.clone(),
        consumer_step_id: "input".into(),
        consumer_region_id: None,
        target_port: "invocation".into(),
        path: Vec::new(),
        expected_type: request.value_type.clone(),
        expected_schema_digest: request.value_schema_digest.clone(),
        required: true,
        mode: WorkflowVariableReadMode::DirectValue,
    });
    let contract = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
        ..spec
    })
    .expect("valid input-read contract");
    let error = validate_runtime_variable_contract(&contract, None, &input.plan)
        .expect_err("runtime must reject projection into Input");
    assert!(error.contains("input step"));
}

#[test]
fn runtime_v2_explicit_reads_cannot_bypass_the_typed_projection() {
    let mut input = typed_variable_workflow_run_input().expect("valid v2 WorkflowRun input");
    let contract = input
        .variable_contract
        .as_ref()
        .expect("variable contract")
        .restore()
        .expect("restored variable contract");
    let mut spec = contract.spec().clone();
    let request = spec
        .declarations
        .iter()
        .find(|declaration| declaration.name == "request")
        .expect("request declaration");
    spec.reads.push(WorkflowVariableRead {
        id: "high-request".into(),
        variable: request.name.clone(),
        consumer_step_id: "high".into(),
        consumer_region_id: None,
        target_port: "request".into(),
        path: Vec::new(),
        expected_type: request.value_type.clone(),
        expected_schema_digest: request.value_schema_digest.clone(),
        required: true,
        mode: WorkflowVariableReadMode::DirectValue,
    });
    let contract = WorkflowVariableContract::from_spec(spec).expect("bypass test contract");
    input.plan.variable_contract_digest = Some(contract.digest().clone());
    input.variable_contract = Some(ResolvedWorkflowVariableContract::from_contract(&contract));
    refresh_plan_digest(&mut input);

    let error = input
        .validate()
        .expect_err("legacy template token must not bypass typed reads");
    assert!(error.contains("bypasses their typed projection"));
}

#[test]
fn human_decision_run_requires_an_exact_form_release_binding() {
    let input = human_decision_workflow_run_input().expect("valid human-decision input");
    input.validate().expect("human-decision input");

    let mut missing = input.clone();
    missing
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == TEST_HUMAN_STEP_ID)
        .expect("human-decision step")
        .capability = None;
    refresh_plan_digest(&mut missing);
    assert!(missing.validate().is_err());

    let mut floating_release = input;
    floating_release
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == TEST_HUMAN_STEP_ID)
        .and_then(|step| step.capability.as_mut())
        .expect("FormRelease capability")
        .revision = "latest".into();
    refresh_plan_digest(&mut floating_release);
    assert!(floating_release.validate().is_err());
}

#[test]
fn workflow_run_timeout_is_strictly_bounded() {
    assert_eq!(
        workflow_run_timeout_seconds(None).expect("default timeout"),
        WORKFLOW_RUN_DEFAULT_TIMEOUT_SECONDS
    );
    assert_eq!(workflow_run_timeout_seconds(Some(1)).expect("minimum"), 1);
    assert_eq!(
        workflow_run_timeout_seconds(Some(WORKFLOW_RUN_MAX_TIMEOUT_SECONDS)).expect("maximum"),
        WORKFLOW_RUN_MAX_TIMEOUT_SECONDS
    );
    assert!(workflow_run_timeout_seconds(Some(0)).is_err());
    assert!(workflow_run_timeout_seconds(Some(WORKFLOW_RUN_MAX_TIMEOUT_SECONDS + 1)).is_err());
}

fn refresh_plan_digest(input: &mut WorkflowRunInput) {
    input.plan_digest = Sha256Digest::parse(sha256_digest(
        &canonical_json_bounded(
            &input.plan,
            WORKFLOW_PLAN_MAX_BYTES,
            "WorkflowRun test plan",
        )
        .expect("canonical plan"),
    ))
    .expect("plan digest");
}

fn test_digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64)))
        .expect("test digest")
}
