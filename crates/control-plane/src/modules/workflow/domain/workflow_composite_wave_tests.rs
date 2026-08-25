use super::*;
use serde_json::json;

fn wave() -> (
    workflow_composite_frame_tests::Fixture,
    WorkflowCompositeWaveHookMetadata,
) {
    let fixture = workflow_composite_frame_tests::fixture();
    let request = WorkflowCompositeWaveRequest {
        organization_id: fixture.request.organization_id,
        project_id: fixture.request.project_id,
        workflow_run_id: fixture.request.workflow_run_id,
        plan_revision_id: fixture.request.plan_revision_id,
        plan_digest: fixture.request.plan_digest.clone(),
        region_step_id: fixture.request.region_step_id.clone(),
        first_ordinal: 0,
        effective_inputs: vec![json!({"item": "A"}), json!({"item": "B"})],
        available_variables: fixture.request.available_variables.clone(),
    };
    let metadata = WorkflowCompositeWaveHookMetadata::new(
        request,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .expect("composite wave");
    (fixture, metadata)
}

#[test]
fn composite_wave_reconstructs_exact_frames_and_flow_identity() {
    let (fixture, wave) = wave();
    let frames = wave
        .frames(&fixture.plan, &fixture.regions, &fixture.variables, None)
        .expect("wave frames");

    assert_eq!(wave.frame_count(), 2);
    assert_eq!(wave.last_ordinal().expect("last ordinal"), 1);
    assert_eq!(wave.flow_hook_id(), "workflow-composite-wave:iteration:0:2");
    assert_eq!(frames[0].ordinal, 0);
    assert_eq!(frames[0].child_input, json!({"item": "A"}));
    assert_eq!(frames[1].ordinal, 1);
    assert_eq!(frames[1].child_input, json!({"item": "B"}));

    let encoded = serde_json::to_value(&wave).expect("wave JSON");
    let restored =
        serde_json::from_value::<WorkflowCompositeWaveHookMetadata>(encoded).expect("restore wave");
    assert_eq!(restored, wave);

    let mut drifted = wave;
    drifted.effective_inputs[1] = json!({"item": "C"});
    assert!(drifted
        .validate(&fixture.plan, &fixture.regions, &fixture.variables, None,)
        .is_err());
}

#[test]
fn composite_wave_resume_is_sorted_and_bound_to_every_frame() {
    let (fixture, wave) = wave();
    let frames = wave
        .frames(&fixture.plan, &fixture.regions, &fixture.variables, None)
        .expect("wave frames");
    let first = frames[0]
        .resolve(
            &fixture.plan,
            &fixture.regions,
            &fixture.variables,
            json!("A!"),
        )
        .expect("first result");
    let second = frames[1]
        .resolve(
            &fixture.plan,
            &fixture.regions,
            &fixture.variables,
            json!("B!"),
        )
        .expect("second result");
    let payload = WorkflowCompositeWaveResumePayload::new(
        &wave,
        vec![
            WorkflowCompositeWaveFrameResolution::completed(&frames[1], second),
            WorkflowCompositeWaveFrameResolution::completed(&frames[0], first),
        ],
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .expect("wave payload");

    assert_eq!(payload.resolutions[0].ordinal(), 0);
    assert_eq!(payload.resolutions[1].ordinal(), 1);
    let bound = payload
        .frame_resolutions(
            &wave,
            &fixture.plan,
            &fixture.regions,
            &fixture.variables,
            None,
        )
        .expect("bound resolutions");
    assert_eq!(bound[0].ordinal(), 0);
    assert_eq!(bound[1].ordinal(), 1);

    let mut drifted = payload;
    drifted.resolutions.swap(0, 1);
    assert!(drifted
        .validate(
            &wave,
            &fixture.plan,
            &fixture.regions,
            &fixture.variables,
            None,
        )
        .is_err());
}

#[test]
fn composite_wave_preserves_primary_failure_after_lower_ordinal_sibling_cancellation() {
    let (fixture, wave) = wave();
    let frames = wave
        .frames(&fixture.plan, &fixture.regions, &fixture.variables, None)
        .expect("wave frames");
    let payload = WorkflowCompositeWaveResumePayload::new(
        &wave,
        vec![
            WorkflowCompositeWaveFrameResolution::failed(&frames[1], "primary failure"),
            WorkflowCompositeWaveFrameResolution::cancelled_after_primary_failure(&frames[0]),
        ],
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .expect("failure-preserving wave payload");

    assert_eq!(payload.resolutions[0].ordinal(), 0);
    assert_eq!(
        payload
            .resolutions
            .iter()
            .find_map(WorkflowCompositeWaveFrameResolution::primary_failure),
        Some((1, "primary failure"))
    );
    assert!(WorkflowCompositeWaveResumePayload::new(
        &wave,
        frames
            .iter()
            .map(WorkflowCompositeWaveFrameResolution::cancelled_after_primary_failure)
            .collect(),
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .is_err());
}

#[test]
fn composite_wave_contracts_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkflowCompositeWaveHookMetadata>();
    assert_send_sync::<WorkflowCompositeWaveFrameResolution>();
    assert_send_sync::<WorkflowCompositeWaveResumePayload>();
}
