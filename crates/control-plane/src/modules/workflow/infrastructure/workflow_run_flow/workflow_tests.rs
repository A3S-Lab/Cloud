use super::*;
use crate::modules::shared_kernel::domain::{Sha256Digest, WorkflowRunId};
use crate::modules::workflow::domain::WORKFLOW_RUN_OUTPUT_MAX_BYTES;
use crate::modules::workflow::domain::{WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION};
use crate::modules::workflow::test_support::{
    default_output_execution_workflow_run_input, digest, multi_output_workflow_run_input,
    timestamp, workflow_run_input, TEST_EXECUTION_STEP_ID,
};
use a3s_flow::{CancellationRequest, FlowEvent, FlowEventEnvelope, WorkflowSpec};
use uuid::Uuid;

#[test]
fn effective_input_is_stable_for_zero_one_and_many_dependencies() {
    let workflow_input = serde_json::json!({"input": true});
    assert_eq!(
        effective_input(&BTreeMap::new(), &workflow_input),
        workflow_input
    );
    assert_eq!(
        effective_input(
            &BTreeMap::from([("a".into(), serde_json::json!(1))]),
            &Value::Null
        ),
        serde_json::json!(1)
    );
    assert_eq!(
        effective_input(
            &BTreeMap::from([
                ("b".into(), serde_json::json!(2)),
                ("a".into(), serde_json::json!(1)),
            ]),
            &Value::Null,
        ),
        serde_json::json!({"a": 1, "b": 2})
    );
}

#[test]
fn terminal_execution_dispatch_failure_folds_into_exact_default_output() {
    let input = default_output_execution_workflow_run_input().expect("default-output input");
    let steps = input.resolved_steps().expect("resolved steps");
    let step = steps
        .iter()
        .find(|step| step.plan.id == TEST_EXECUTION_STEP_ID)
        .expect("Execution step");
    let metadata = WorkflowExecutionHookMetadata::from_run_step(
        &input,
        step,
        serde_json::json!({"command": "verify"}),
    )
    .expect("hook metadata");
    let payload =
        WorkflowExecutionResumePayload::rejected(&metadata, "provider capacity exhausted")
            .expect("rejected payload");
    let observed = serde_json::to_value(payload).expect("encoded payload");
    let result = execution_result(
        &input.workflow_run_id.to_string(),
        &metadata.flow_hook_id(),
        &input,
        step,
        &metadata,
        &observed,
    )
    .expect("default-output resolution");
    let ExecutionResolution::Succeeded(result) = result else {
        panic!("default-output fallback must remain a successful graph value");
    };
    assert_eq!(
        result.output,
        serde_json::json!({"status": "temporarily_unavailable"})
    );
    assert!(result.selected_handle.is_none());
    let evidence = result
        .default_output_evidence
        .as_ref()
        .expect("terminal failure evidence");
    assert_eq!(
        evidence.failure.classification,
        crate::modules::workflow::domain::WorkflowStepFailureClassification::DispatchRejected
    );
    evidence.validate(step).expect("authority-bound evidence");
    result.validate(step).expect("validated folded output");
}

#[test]
fn workflow_runtime_prioritizes_cancellation_and_enforces_deadline() {
    let input = workflow_run_input().expect("WorkflowRun input");
    let cancellation = invocation(
        &input,
        vec![envelope(
            &input,
            1,
            timestamp(8, 5),
            FlowEvent::RunCancellationRequested {
                request: CancellationRequest::new(Some("operator request".into())),
            },
        )],
    );
    assert_eq!(
        run_workflow(cancellation).expect("cancellation command"),
        RuntimeCommand::Cancel
    );

    let timeout = invocation(
        &input,
        vec![envelope(
            &input,
            1,
            input.deadline_at,
            FlowEvent::RunStarted,
        )],
    );
    assert_eq!(
        run_workflow(timeout).expect("timeout command"),
        RuntimeCommand::Timeout {
            deadline: input.deadline_at,
            reason: Some("WorkflowRun exceeded its immutable deadline".into()),
        }
    );
}

#[test]
fn workflow_runtime_rejects_identity_and_replayed_step_drift() {
    let input = workflow_run_input().expect("WorkflowRun input");
    let mut identity_drift = invocation(&input, Vec::new());
    identity_drift.run_id = WorkflowRunId::new().to_string();
    assert!(matches!(
        run_workflow(identity_drift),
        Err(FlowError::NonDeterministic { .. })
    ));

    let drifted_result = WorkflowLocalStepResult {
        step_id: "input".into(),
        kind: WorkflowStepKind::Input,
        output: input.goal_input.clone(),
        output_digest: Sha256Digest::parse(digest('f')).expect("digest"),
        selected_handle: None,
        composite_region_result: None,
        default_output_evidence: None,
    };
    let replay_drift = invocation(
        &input,
        vec![envelope(
            &input,
            1,
            timestamp(8, 1),
            FlowEvent::StepCompleted {
                step_id: flow_step_id("input"),
                output: serde_json::to_value(drifted_result).expect("step result"),
            },
        )],
    );
    assert!(matches!(
        run_workflow(replay_drift),
        Err(FlowError::NonDeterministic { .. })
    ));
}

#[test]
fn multi_output_aggregate_enforces_the_workflow_run_output_bound() {
    let input = multi_output_workflow_run_input().expect("multi-output WorkflowRun input");
    let value = Value::String("x".repeat(WORKFLOW_RUN_OUTPUT_MAX_BYTES / 2));
    let resolved = BTreeMap::from([
        (
            "output-a".into(),
            ResolvedState::Active(Box::new(WorkflowLocalStepResult {
                step_id: "output-a".into(),
                kind: WorkflowStepKind::Output,
                output: value.clone(),
                output_digest: Sha256Digest::parse(digest('a')).expect("digest"),
                selected_handle: None,
                composite_region_result: None,
                default_output_evidence: None,
            })),
        ),
        (
            "output-b".into(),
            ResolvedState::Active(Box::new(WorkflowLocalStepResult {
                step_id: "output-b".into(),
                kind: WorkflowStepKind::Output,
                output: value,
                output_digest: Sha256Digest::parse(digest('b')).expect("digest"),
                selected_handle: None,
                composite_region_result: None,
                default_output_evidence: None,
            })),
        ),
    ]);

    assert!(resolved_workflow_output(&input, &resolved)
        .expect_err("oversized aggregate")
        .contains("exceeds its 262144-byte bound"));
}

fn invocation(input: &WorkflowRunInput, history: Vec<FlowEventEnvelope>) -> WorkflowInvocation {
    WorkflowInvocation::new(
        input.workflow_run_id.to_string(),
        WorkflowSpec::rust_embedded(
            WORKFLOW_RUN_FLOW_NAME,
            WORKFLOW_RUN_FLOW_VERSION,
            "cloud",
            "workflow_run",
        ),
        serde_json::to_value(input).expect("WorkflowRun input JSON"),
        history,
    )
}

fn envelope(
    input: &WorkflowRunInput,
    sequence: u64,
    timestamp: chrono::DateTime<chrono::Utc>,
    event: FlowEvent,
) -> FlowEventEnvelope {
    FlowEventEnvelope::new(
        input.workflow_run_id.to_string(),
        sequence,
        Uuid::now_v7(),
        timestamp,
        event,
    )
}
