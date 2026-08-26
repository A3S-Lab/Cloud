use super::*;
use crate::modules::shared_kernel::domain::{
    AgentConversationId, AgentExecutionId, OperationId, Sha256Digest, WorkflowRunId,
};
use crate::modules::workflow::domain::WORKFLOW_RUN_OUTPUT_MAX_BYTES;
use crate::modules::workflow::domain::{
    WorkflowAgentHookMetadata, WorkflowAgentOutcome, WorkflowAgentProviderEvidence,
    WorkflowAgentResumePayload, WorkflowAgentStepOutput, WORKFLOW_AGENT_RESULT_SCHEMA,
    WORKFLOW_RUN_FLOW_NAME, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V5,
    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V7,
};
use crate::modules::workflow::infrastructure::workflow_run_flow::execution;
use crate::modules::workflow::test_support::{
    agent_workflow_run_input, branch_failure_workflow_run_input,
    default_output_execution_workflow_run_input, digest, multi_output_workflow_run_input,
    output_failure_workflow_run_input, timestamp, transform_failure_workflow_run_input,
    workflow_run_input, TEST_AGENT_STEP_ID, TEST_EXECUTION_STEP_ID,
};
use a3s_flow::{
    CancellationRequest, FlowEvent, FlowEventEnvelope, StepFailureAction, WorkflowSpec,
};
use uuid::Uuid;

#[test]
fn agent_hook_and_terminal_result_are_exact_and_replay_safe() {
    let input = agent_workflow_run_input().expect("Agent WorkflowRun input");
    let resolved = input.resolved_steps().expect("resolved Agent plan");
    let agent_step = resolved
        .iter()
        .find(|step| step.plan.id == TEST_AGENT_STEP_ID)
        .expect("Agent step");
    let input_result = WorkflowLocalStepResult {
        step_id: "input".into(),
        kind: WorkflowStepKind::Input,
        output: input.goal_input.clone(),
        output_digest: execution::value_digest(&input.goal_input, "Agent test input")
            .expect("input digest"),
        selected_handle: None,
        composite_region_result: None,
        default_output_evidence: None,
    };
    let command = run_workflow(invocation(
        &input,
        vec![envelope(
            &input,
            1,
            timestamp(8, 1),
            FlowEvent::StepCompleted {
                step_id: flow_step_id("input"),
                output: serde_json::to_value(input_result).expect("input result"),
            },
        )],
    ))
    .expect("Agent hook command");
    let RuntimeCommand::CreateHook {
        hook_id,
        token,
        metadata,
        ..
    } = command
    else {
        panic!("Agent step must suspend on an exact hook");
    };
    let observed = serde_json::from_value::<WorkflowAgentHookMetadata>(metadata)
        .expect("typed Agent hook metadata");
    let expected =
        WorkflowAgentHookMetadata::from_run_step(&input, agent_step, input.goal_input.clone())
            .expect("expected Agent hook metadata");
    assert_eq!(observed, expected);
    assert_eq!(hook_id, expected.flow_hook_id());
    assert_eq!(token, expected.flow_hook_token());

    let output = WorkflowAgentStepOutput {
        schema: WORKFLOW_AGENT_RESULT_SCHEMA.into(),
        conversation_id: AgentConversationId::new(),
        agent_execution_id: AgentExecutionId::new(),
        operation_id: OperationId::new(),
        agent_asset_id: expected.agent_asset_id,
        agent_asset_release_id: expected.agent_asset_release_id,
        agent_release_digest: expected.agent_release_digest.clone(),
        provider: Some(WorkflowAgentProviderEvidence {
            kind: "a3s-code".into(),
            revision: "1".into(),
            protocol: "agent.provider.v1".into(),
            native_protocol: "agent.protocol.v1".into(),
            profile_digest: Sha256Digest::parse(digest('1')).expect("profile digest"),
            capability_digest: Sha256Digest::parse(digest('2')).expect("capability digest"),
            session_id: "session-1".into(),
            run_id: "run-1".into(),
        }),
        outcome: WorkflowAgentOutcome::Succeeded,
        text: "hello from the Agent".into(),
        terminal_event_sequence: 3,
        finished_at: timestamp(8, 2),
    };
    let payload =
        WorkflowAgentResumePayload::new(&expected, output.clone()).expect("Agent resume payload");
    let encoded = serde_json::to_value(payload).expect("encoded Agent resume payload");
    let resolution = agent_result(
        &input.workflow_run_id.to_string(),
        &hook_id,
        agent_step,
        &expected,
        &encoded,
    )
    .expect("Agent terminal result");
    let AgentResolution::Succeeded(result) = resolution else {
        panic!("successful Agent terminal output must resolve the step");
    };
    assert_eq!(result.kind, WorkflowStepKind::Agent);
    assert_eq!(
        result.output,
        serde_json::to_value(output).expect("encoded Agent output")
    );

    let rejected = WorkflowAgentResumePayload::rejected(&expected, "release is unavailable")
        .expect("Agent rejection payload");
    assert!(matches!(
        agent_result(
            &input.workflow_run_id.to_string(),
            &hook_id,
            agent_step,
            &expected,
            &serde_json::to_value(rejected).expect("encoded Agent rejection"),
        )
        .expect("Agent rejection result"),
        AgentResolution::Failed(reason) if reason == "release is unavailable"
    ));

    let mut drifted = encoded;
    drifted["flowHookId"] = serde_json::json!("workflow-agent:other:1");
    assert!(matches!(
        agent_result(
            &input.workflow_run_id.to_string(),
            &hook_id,
            agent_step,
            &expected,
            &drifted,
        ),
        Err(FlowError::NonDeterministic { .. })
    ));
}

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
fn transform_failure_routes_redacted_output_without_retrying() {
    let input = transform_failure_workflow_run_input().expect("routed Transform input");
    let input_result = WorkflowLocalStepResult {
        step_id: "input".into(),
        kind: WorkflowStepKind::Input,
        output: input.goal_input.clone(),
        output_digest: execution::value_digest(&input.goal_input, "test input").expect("digest"),
        selected_handle: None,
        composite_region_result: None,
        default_output_evidence: None,
    };
    let input_completed = envelope(
        &input,
        1,
        timestamp(8, 1),
        FlowEvent::StepCompleted {
            step_id: flow_step_id("input"),
            output: serde_json::to_value(input_result).expect("input result"),
        },
    );
    let scheduled = run_workflow(invocation(&input, vec![input_completed.clone()]))
        .expect("Transform scheduling");
    let RuntimeCommand::ScheduleSteps { steps } = scheduled else {
        panic!("routed Transform must be scheduled as a recoverable local step");
    };
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].step_id, flow_step_id(TEST_EXECUTION_STEP_ID));
    assert_eq!(steps[0].retry.max_attempts, 1);
    assert_eq!(
        steps[0].retry.on_exhausted,
        StepFailureAction::ContinueWorkflow
    );

    let failed = envelope(
        &input,
        2,
        timestamp(8, 2),
        FlowEvent::StepFailed {
            step_id: flow_step_id(TEST_EXECUTION_STEP_ID),
            attempt: 1,
            error: "runtime error: secret template detail".into(),
        },
    );
    let routed = run_workflow(invocation(&input, vec![input_completed, failed]))
        .expect("Transform failure routing");
    let RuntimeCommand::ScheduleSteps { steps } = routed else {
        panic!("routed Transform failure must schedule its error sink");
    };
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].step_id, flow_step_id("failure_output"));
    let sink_input = serde_json::from_value::<WorkflowLocalStepInput>(steps[0].input.clone())
        .expect("failure sink input");
    let failure = serde_json::from_value::<WorkflowStepFailureOutput>(sink_input.effective_input)
        .expect("typed Transform failure");
    assert_eq!(failure.schema, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V5);
    assert_eq!(
        failure.classification,
        WorkflowStepFailureClassification::WorkflowLocalInvalid
    );
    assert!(!failure.message.contains("secret"));
}

#[test]
fn output_failure_routes_redacted_output_without_retrying() {
    let input = output_failure_workflow_run_input().expect("routed Output input");
    let input_result = WorkflowLocalStepResult {
        step_id: "input".into(),
        kind: WorkflowStepKind::Input,
        output: input.goal_input.clone(),
        output_digest: execution::value_digest(&input.goal_input, "test input").expect("digest"),
        selected_handle: None,
        composite_region_result: None,
        default_output_evidence: None,
    };
    let input_completed = envelope(
        &input,
        1,
        timestamp(8, 1),
        FlowEvent::StepCompleted {
            step_id: flow_step_id("input"),
            output: serde_json::to_value(input_result).expect("input result"),
        },
    );
    let scheduled =
        run_workflow(invocation(&input, vec![input_completed.clone()])).expect("Output scheduling");
    let RuntimeCommand::ScheduleSteps { steps } = scheduled else {
        panic!("routed Output must be scheduled as a recoverable local step");
    };
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].step_id, flow_step_id(TEST_EXECUTION_STEP_ID));
    assert_eq!(steps[0].retry.max_attempts, 1);
    assert_eq!(
        steps[0].retry.on_exhausted,
        StepFailureAction::ContinueWorkflow
    );

    let failed = envelope(
        &input,
        2,
        timestamp(8, 2),
        FlowEvent::StepFailed {
            step_id: flow_step_id(TEST_EXECUTION_STEP_ID),
            attempt: 1,
            error: "runtime error: secret output detail".into(),
        },
    );
    let routed = run_workflow(invocation(&input, vec![input_completed, failed]))
        .expect("Output failure routing");
    let RuntimeCommand::ScheduleSteps { steps } = routed else {
        panic!("routed Output failure must schedule its error sink");
    };
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].step_id, flow_step_id("failure_output"));
    let sink_input = serde_json::from_value::<WorkflowLocalStepInput>(steps[0].input.clone())
        .expect("failure sink input");
    let failure = serde_json::from_value::<WorkflowStepFailureOutput>(sink_input.effective_input)
        .expect("typed Output failure");
    assert_eq!(failure.schema, "cloud.workflow.step-failure.v6");
    assert_eq!(
        failure.classification,
        WorkflowStepFailureClassification::WorkflowLocalInvalid
    );
    assert!(!failure.message.contains("secret"));
}

#[test]
fn branch_failure_routes_redacted_output_without_retrying() {
    let input = branch_failure_workflow_run_input().expect("routed Branch input");
    let input_result = WorkflowLocalStepResult {
        step_id: "input".into(),
        kind: WorkflowStepKind::Input,
        output: input.goal_input.clone(),
        output_digest: execution::value_digest(&input.goal_input, "test input").expect("digest"),
        selected_handle: None,
        composite_region_result: None,
        default_output_evidence: None,
    };
    let input_completed = envelope(
        &input,
        1,
        timestamp(8, 1),
        FlowEvent::StepCompleted {
            step_id: flow_step_id("input"),
            output: serde_json::to_value(input_result).expect("input result"),
        },
    );
    let scheduled =
        run_workflow(invocation(&input, vec![input_completed.clone()])).expect("Branch scheduling");
    let RuntimeCommand::ScheduleSteps { steps } = scheduled else {
        panic!("routed Branch must be scheduled as a recoverable local step");
    };
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].step_id, flow_step_id(TEST_EXECUTION_STEP_ID));
    assert_eq!(steps[0].retry.max_attempts, 1);
    assert_eq!(
        steps[0].retry.on_exhausted,
        StepFailureAction::ContinueWorkflow
    );

    let failed = envelope(
        &input,
        2,
        timestamp(8, 2),
        FlowEvent::StepFailed {
            step_id: flow_step_id(TEST_EXECUTION_STEP_ID),
            attempt: 1,
            error: "runtime error: secret selector detail".into(),
        },
    );
    let routed = run_workflow(invocation(&input, vec![input_completed, failed]))
        .expect("Branch failure routing");
    let RuntimeCommand::ScheduleSteps { steps } = routed else {
        panic!("routed Branch failure must schedule its error sink");
    };
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].step_id, flow_step_id("failure_output"));
    let sink_input = serde_json::from_value::<WorkflowLocalStepInput>(steps[0].input.clone())
        .expect("failure sink input");
    let failure = serde_json::from_value::<WorkflowStepFailureOutput>(sink_input.effective_input)
        .expect("typed Branch failure");
    assert_eq!(failure.schema, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V7);
    assert_eq!(
        failure.classification,
        WorkflowStepFailureClassification::WorkflowLocalInvalid
    );
    assert_eq!(failure.message, "Workflow Branch evaluation was invalid");
    assert!(!failure.message.contains("secret"));
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
            input.flow_workflow_version.clone(),
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
