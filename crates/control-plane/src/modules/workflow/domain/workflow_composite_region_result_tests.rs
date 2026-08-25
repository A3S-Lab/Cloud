use super::workflow_composite_frame_tests::{fixture, plan_digest, Fixture};
use super::*;
use serde_json::{json, Value};

fn request(fixture: &Fixture) -> WorkflowCompositeRegionResultRequest {
    WorkflowCompositeRegionResultRequest {
        organization_id: fixture.request.organization_id,
        project_id: fixture.request.project_id,
        workflow_run_id: fixture.request.workflow_run_id,
        plan_revision_id: fixture.request.plan_revision_id,
        plan_digest: fixture.request.plan_digest.clone(),
        region_step_id: fixture.request.region_step_id.clone(),
    }
}

fn completed(
    fixture: &Fixture,
    ordinal: u32,
    effective_input: Value,
    child_output: Value,
) -> WorkflowCompositeFrameResolution {
    let mut frame_request = fixture.request.clone();
    frame_request.ordinal = ordinal;
    frame_request.effective_input = effective_input;
    let frame = WorkflowCompositeFrame::open(
        frame_request,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .expect("frame");
    let result = frame
        .resolve(
            &fixture.plan,
            &fixture.regions,
            &fixture.variables,
            child_output,
        )
        .expect("frame result");
    WorkflowCompositeFrameResolution::completed(frame, result)
}

fn failed(
    fixture: &Fixture,
    ordinal: u32,
    effective_input: Value,
    error: &str,
) -> WorkflowCompositeFrameResolution {
    let mut frame_request = fixture.request.clone();
    frame_request.ordinal = ordinal;
    frame_request.effective_input = effective_input;
    let frame = WorkflowCompositeFrame::open(
        frame_request,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .expect("frame");
    WorkflowCompositeFrameResolution::failed(frame, error)
}

fn iteration_fixture(failure_mode: WorkflowIterationFailureMode) -> Fixture {
    let mut fixture = fixture();
    fixture.regions = WorkflowCompositeRegions::from_spec(WorkflowCompositeRegionsSpec {
        id: "support.composite".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
        regions: vec![WorkflowCompositeRegionPolicy::Iteration(
            WorkflowIterationRegionPolicy {
                step_id: "iteration".into(),
                maximum_items: 2,
                maximum_concurrency: 2,
                failure_mode,
            },
        )],
    })
    .expect("iteration regions");
    fixture.plan.composite_regions_digest = Some(fixture.regions.digest().clone());
    fixture.request.plan_digest = plan_digest(&fixture.plan);
    fixture
}

fn loop_fixture() -> Fixture {
    let mut fixture = fixture();
    fixture.regions = WorkflowCompositeRegions::from_spec(WorkflowCompositeRegionsSpec {
        id: "support.composite".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
        regions: vec![WorkflowCompositeRegionPolicy::Loop(
            WorkflowLoopRegionPolicy {
                step_id: "iteration".into(),
                maximum_iterations: 3,
                time_budget_seconds: 60,
                termination_path: vec!["done".into()],
            },
        )],
    })
    .expect("loop regions");
    fixture
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == "iteration")
        .and_then(|step| step.descriptor.as_mut())
        .expect("loop descriptor")
        .descriptor_id = "workflow.loop".into();

    let mut variable_spec = fixture.variables.spec().clone();
    for declaration in &mut variable_spec.declarations {
        if matches!(
            declaration.name.as_str(),
            "raw_result" | "normalized" | "iteration_result" | "summary"
        ) {
            declaration.value_type = WorkflowDataType::Object;
        }
    }
    for assignment in &mut variable_spec.assignments {
        assignment.value_type = WorkflowDataType::Object;
    }
    for export in &mut variable_spec.exports {
        export.value_type = WorkflowDataType::Object;
    }
    fixture.variables = WorkflowVariableContract::from_spec(variable_spec).expect("loop variables");
    fixture.plan.variable_contract_digest = Some(fixture.variables.digest().clone());
    fixture.plan.composite_regions_digest = Some(fixture.regions.digest().clone());
    fixture.request.plan_digest = plan_digest(&fixture.plan);
    fixture
}

#[test]
fn iteration_reconstructs_completion_order_by_stable_ordinal() {
    let fixture = iteration_fixture(WorkflowIterationFailureMode::Terminate);
    let second = completed(&fixture, 1, json!({"item": "B"}), json!("B!"));
    let first = completed(&fixture, 0, json!({"item": "A"}), json!("A!"));

    let result = WorkflowCompositeRegionResult::resolve_iteration(
        request(&fixture),
        2,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        vec![second, first],
    )
    .expect("ordered iteration result");

    assert_eq!(result.output, json!(["A!", "B!"]));
    assert_eq!(result.frames[0].ordinal(), 0);
    assert_eq!(result.frames[1].ordinal(), 1);
    assert_eq!(
        result.run_variable_updates.get("summary"),
        Some(&json!("B!"))
    );
    assert_eq!(
        result.exported_variables.get("iteration_result"),
        Some(&json!("B!"))
    );
    result
        .validate(&fixture.plan, &fixture.regions, &fixture.variables)
        .expect("valid ordered result");

    let restored: WorkflowCompositeRegionResult =
        serde_json::from_value(serde_json::to_value(&result).expect("region result JSON"))
            .expect("restored region result");
    assert_eq!(restored, result);
}

#[test]
fn iteration_applies_each_immutable_failure_mode_without_losing_ordinals() {
    let continue_null = iteration_fixture(WorkflowIterationFailureMode::ContinueNull);
    let continued = WorkflowCompositeRegionResult::resolve_iteration(
        request(&continue_null),
        2,
        &continue_null.plan,
        &continue_null.regions,
        &continue_null.variables,
        vec![
            completed(&continue_null, 0, json!({"item": "A"}), json!("A!")),
            failed(&continue_null, 1, json!({"item": "B"}), "child rejected B"),
        ],
    )
    .expect("continue-null result");
    assert_eq!(continued.output, json!(["A!", null]));
    assert_eq!(continued.frames.len(), 2);

    let remove_failed = iteration_fixture(WorkflowIterationFailureMode::RemoveFailed);
    let removed = WorkflowCompositeRegionResult::resolve_iteration(
        request(&remove_failed),
        2,
        &remove_failed.plan,
        &remove_failed.regions,
        &remove_failed.variables,
        vec![
            completed(&remove_failed, 0, json!({"item": "A"}), json!("A!")),
            failed(&remove_failed, 1, json!({"item": "B"}), "child rejected B"),
        ],
    )
    .expect("remove-failed result");
    assert_eq!(removed.output, json!(["A!"]));
    assert_eq!(removed.frames.len(), 2);

    let terminate = iteration_fixture(WorkflowIterationFailureMode::Terminate);
    let error = WorkflowCompositeRegionResult::resolve_iteration(
        request(&terminate),
        2,
        &terminate.plan,
        &terminate.regions,
        &terminate.variables,
        vec![
            completed(&terminate, 0, json!({"item": "A"}), json!("A!")),
            failed(&terminate, 1, json!({"item": "B"}), "child rejected B"),
        ],
    )
    .expect_err("terminate failure mode");
    assert!(error.contains("frame 1 failed"));
}

#[test]
fn empty_iteration_has_one_authority_bound_empty_result() {
    let fixture = iteration_fixture(WorkflowIterationFailureMode::Terminate);
    let result = WorkflowCompositeRegionResult::resolve_iteration(
        request(&fixture),
        0,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        Vec::new(),
    )
    .expect("empty iteration");
    assert_eq!(result.output, json!([]));
    assert!(result.frames.is_empty());
    assert!(result.run_variable_updates.is_empty());
    assert!(result.exported_variables.is_empty());
}

#[test]
fn loop_requires_one_boolean_terminal_frame_and_keeps_its_output() {
    let fixture = loop_fixture();
    let second_output = json!({"done": true, "item": "B", "value": "B!"});
    let result = WorkflowCompositeRegionResult::resolve_loop(
        request(&fixture),
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        vec![
            completed(&fixture, 1, json!({"item": "B"}), second_output.clone()),
            completed(
                &fixture,
                0,
                json!({"item": "A"}),
                json!({"done": false, "item": "A", "value": "A!"}),
            ),
        ],
    )
    .expect("terminated loop");
    assert_eq!(result.output, second_output);
    assert_eq!(
        result.run_variable_updates.get("summary"),
        Some(&json!({"done": true, "item": "B", "value": "B!"}))
    );

    let unfinished = WorkflowCompositeRegionResult::resolve_loop(
        request(&fixture),
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        vec![completed(
            &fixture,
            0,
            json!({"item": "A"}),
            json!({"done": false, "item": "A"}),
        )],
    )
    .expect_err("unterminated loop");
    assert!(unfinished.contains("before its termination condition"));

    let extra = WorkflowCompositeRegionResult::resolve_loop(
        request(&fixture),
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        vec![
            completed(
                &fixture,
                0,
                json!({"item": "A"}),
                json!({"done": true, "item": "A"}),
            ),
            completed(
                &fixture,
                1,
                json!({"item": "B"}),
                json!({"done": true, "item": "B"}),
            ),
        ],
    )
    .expect_err("frames after termination");
    assert!(extra.contains("after its termination condition"));
}

#[test]
fn region_result_rejects_gaps_invalid_failures_and_digest_tampering() {
    let fixture = iteration_fixture(WorkflowIterationFailureMode::ContinueNull);
    let gap = WorkflowCompositeRegionResult::resolve_iteration(
        request(&fixture),
        1,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        vec![completed(&fixture, 1, json!({"item": "B"}), json!("B!"))],
    )
    .expect_err("ordinal gap");
    assert!(gap.contains("contiguous"));

    let invalid_failure = WorkflowCompositeRegionResult::resolve_iteration(
        request(&fixture),
        1,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        vec![failed(&fixture, 0, json!({"item": "A"}), "bad\nerror")],
    )
    .expect_err("invalid failure");
    assert!(invalid_failure.contains("failure is invalid"));

    let mut result = WorkflowCompositeRegionResult::resolve_iteration(
        request(&fixture),
        1,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        vec![completed(&fixture, 0, json!({"item": "A"}), json!("A!"))],
    )
    .expect("region result");
    result.output = json!(["tampered"]);
    assert!(result
        .validate(&fixture.plan, &fixture.regions, &fixture.variables)
        .is_err());
}

#[test]
fn composite_region_result_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkflowCompositeRegionResultRequest>();
    assert_send_sync::<WorkflowCompositeFrameResolution>();
    assert_send_sync::<WorkflowCompositeRegionResult>();
}
