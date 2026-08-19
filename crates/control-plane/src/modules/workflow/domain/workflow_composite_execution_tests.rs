use super::*;
use serde_json::json;

fn hook() -> (
    workflow_composite_frame_tests::Fixture,
    WorkflowCompositeHookMetadata,
) {
    let fixture = workflow_composite_frame_tests::fixture();
    let frame = WorkflowCompositeFrame::open(
        fixture.request.clone(),
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .expect("composite frame");
    let hook = WorkflowCompositeHookMetadata::new(
        frame,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
    )
    .expect("composite hook");
    (fixture, hook)
}

#[test]
fn composite_hook_binds_the_exact_frame_and_flow_identity() {
    let (fixture, hook) = hook();

    assert_eq!(hook.flow_hook_id(), "workflow-composite:iteration:0");
    assert!(hook.flow_hook_token().starts_with("workflow-composite:"));
    hook.validate(&fixture.plan, &fixture.regions, &fixture.variables)
        .expect("valid hook");

    let mut drifted = hook;
    drifted.frame.ordinal = 1;
    assert!(drifted
        .validate(&fixture.plan, &fixture.regions, &fixture.variables)
        .is_err());
}

#[test]
fn composite_resume_payload_is_digest_bound_to_one_resolution() {
    let (fixture, hook) = hook();
    let result = hook
        .frame
        .resolve(
            &fixture.plan,
            &fixture.regions,
            &fixture.variables,
            json!("A!"),
        )
        .expect("frame result");
    let payload = WorkflowCompositeResumePayload::new(
        &hook,
        WorkflowCompositeFrameResolution::completed(hook.frame.clone(), result),
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
    )
    .expect("resume payload");

    payload
        .validate(&hook, &fixture.plan, &fixture.regions, &fixture.variables)
        .expect("valid payload");
    let encoded = serde_json::to_value(&payload).expect("payload JSON");
    let restored =
        serde_json::from_value::<WorkflowCompositeResumePayload>(encoded).expect("restore payload");
    assert_eq!(restored, payload);

    let mut drifted = payload;
    drifted.flow_hook_id.push_str("-drift");
    assert!(drifted
        .validate(&hook, &fixture.plan, &fixture.regions, &fixture.variables)
        .is_err());
}

#[test]
fn composite_resume_rejects_invalid_failure_evidence() {
    let (fixture, hook) = hook();
    assert!(WorkflowCompositeResumePayload::new(
        &hook,
        WorkflowCompositeFrameResolution::failed(hook.frame.clone(), "bad\nfailure"),
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
    )
    .is_err());
}

#[test]
fn composite_execution_contracts_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkflowCompositeHookMetadata>();
    assert_send_sync::<WorkflowCompositeResumePayload>();
    assert_send_sync::<WorkflowCompositeChildReferenceMetadata>();
}
