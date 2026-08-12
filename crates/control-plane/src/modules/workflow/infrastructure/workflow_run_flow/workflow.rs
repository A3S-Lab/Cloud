use super::{
    decode_input, WorkflowLocalStepInput, WorkflowLocalStepResult, WORKFLOW_RUN_STEP_NAME,
};
use crate::modules::workflow::domain::{
    flow_step_id, FlowResumePayload, ResolvedWorkflowRunStep, WorkflowEdgeSpec,
    WorkflowExecutionHookMetadata, WorkflowExecutionResumePayload,
    WorkflowExecutionResumeResolution, WorkflowHumanDecisionHookMetadata, WorkflowRunInput,
    WorkflowStepKind, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION,
};
use a3s_flow::{FlowError, RuntimeCommand, WorkflowInvocation};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
enum ResolvedState {
    Active(WorkflowLocalStepResult),
    Inactive,
}

pub(super) fn run_workflow(invocation: WorkflowInvocation) -> Result<RuntimeCommand, FlowError> {
    let input = decode_input(invocation.input.clone())?;
    if invocation.run_id != input.workflow_run_id.to_string()
        || invocation.spec.name != input.flow_workflow_name
        || invocation.spec.version != input.flow_workflow_version
    {
        return Err(FlowError::NonDeterministic {
            run_id: invocation.run_id,
            reason: "WorkflowRun identity or Flow WorkflowSpec drifted from its immutable input"
                .into(),
        });
    }
    let context = invocation.context();
    if context.cancellation_request().is_some() {
        return Ok(context.cancel());
    }
    if context
        .history()
        .last()
        .is_some_and(|event| event.timestamp >= input.deadline_at)
    {
        return Ok(context.timeout(
            input.deadline_at,
            Some("WorkflowRun exceeded its immutable deadline".into()),
        ));
    }

    let resolved_steps = input.resolved_steps().map_err(|error| {
        FlowError::InvalidWorkflow(format!("invalid WorkflowRun plan: {error}"))
    })?;
    let by_id = resolved_steps
        .iter()
        .map(|step| (step.plan.id.as_str(), step))
        .collect::<BTreeMap<_, _>>();
    let incoming = incoming_edges(&input);
    let mut resolved = BTreeMap::<String, ResolvedState>::new();
    let mut ready = Vec::new();

    for plan_step in &input.plan.steps {
        let step = by_id.get(plan_step.id.as_str()).ok_or_else(|| {
            FlowError::InvalidWorkflow(format!("WorkflowRun lost resolved step {:?}", plan_step.id))
        })?;
        let Some(dependencies) = dependency_state(step, &incoming, &resolved)? else {
            continue;
        };
        let Some(dependencies) = dependencies else {
            resolved.insert(step.plan.id.clone(), ResolvedState::Inactive);
            continue;
        };
        let effective_input = effective_input(&dependencies, &input.goal_input);
        if step.plan.kind == WorkflowStepKind::HumanDecision {
            let metadata = WorkflowHumanDecisionHookMetadata::from_run_step(&input, step)
                .map_err(FlowError::InvalidWorkflow)?;
            let hook_id = metadata.flow_hook_id();
            if context.hook_disposed(&hook_id) {
                return Ok(context.fail(format!(
                    "Workflow human-decision hook for step {:?} was disposed",
                    step.plan.id
                )));
            }
            if let Some(payload) = context.hook_payload(&hook_id) {
                let result = human_decision_result(&invocation.run_id, &hook_id, step, payload)?;
                resolved.insert(step.plan.id.clone(), ResolvedState::Active(result));
                continue;
            }
            return Ok(context.create_hook(
                hook_id,
                metadata.flow_hook_token(),
                serde_json::to_value(metadata)?,
            ));
        }
        if step.plan.kind == WorkflowStepKind::Execution {
            super::execution::validate_data_schema(
                &step.input_schema,
                &effective_input,
                "Workflow execution step input",
            )
            .map_err(FlowError::InvalidWorkflow)?;
            let metadata =
                WorkflowExecutionHookMetadata::from_run_step(&input, step, effective_input.clone())
                    .map_err(FlowError::InvalidWorkflow)?;
            let hook_id = metadata.flow_hook_id();
            if context.hook_disposed(&hook_id) {
                return Ok(context.fail(format!(
                    "Workflow execution hook for step {:?} was disposed",
                    step.plan.id
                )));
            }
            if let Some(payload) = context.hook_payload(&hook_id) {
                match execution_result(&invocation.run_id, &hook_id, step, &metadata, payload)? {
                    ExecutionResolution::Succeeded(result) => {
                        resolved.insert(step.plan.id.clone(), ResolvedState::Active(result));
                        continue;
                    }
                    ExecutionResolution::Failed(error) => {
                        return Ok(context.fail(format!(
                            "Workflow execution step {:?} failed: {error}",
                            step.plan.id
                        )));
                    }
                }
            }
            return Ok(context.create_hook(
                hook_id,
                metadata.flow_hook_token(),
                serde_json::to_value(metadata)?,
            ));
        }
        let durable_step_id = flow_step_id(&step.plan.id);
        if let Some(error) = context.step_failed(&durable_step_id) {
            return Ok(context.fail(format!("Workflow step {:?} failed: {error}", step.plan.id)));
        }
        if let Some(value) = context.step_output(&durable_step_id) {
            let result = serde_json::from_value::<WorkflowLocalStepResult>(value.clone())?;
            result
                .validate(step)
                .map_err(|error| FlowError::NonDeterministic {
                    run_id: invocation.run_id.clone(),
                    reason: format!(
                        "Workflow step {:?} replay result drifted: {error}",
                        step.plan.id
                    ),
                })?;
            if step.plan.kind == WorkflowStepKind::Output {
                return Ok(context.complete(result.output));
            }
            resolved.insert(step.plan.id.clone(), ResolvedState::Active(result));
            continue;
        }
        let all_steps = resolved
            .iter()
            .filter_map(|(id, state)| match state {
                ResolvedState::Active(result) => Some((id.clone(), result.output.clone())),
                ResolvedState::Inactive => None,
            })
            .collect::<BTreeMap<_, _>>();
        let step_input = WorkflowLocalStepInput {
            runtime_contract_revision: WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION.into(),
            step: (*step).clone(),
            workflow_input: input.goal_input.clone(),
            effective_input,
            dependencies,
            steps: all_steps,
        };
        ready.push(context.step(
            durable_step_id,
            WORKFLOW_RUN_STEP_NAME,
            serde_json::to_value(step_input)?,
        ));
    }

    if ready.is_empty() {
        Ok(context.fail("WorkflowRun graph stalled before its output completed"))
    } else {
        Ok(context.schedule_steps(ready))
    }
}

pub(super) enum ExecutionResolution {
    Succeeded(WorkflowLocalStepResult),
    Failed(String),
}

pub(super) fn execution_result(
    run_id: &str,
    hook_id: &str,
    step: &ResolvedWorkflowRunStep,
    metadata: &WorkflowExecutionHookMetadata,
    observed: &Value,
) -> Result<ExecutionResolution, FlowError> {
    let payload = serde_json::from_value::<WorkflowExecutionResumePayload>(observed.clone())
        .map_err(|_| execution_payload_drift(run_id, &step.plan.id))?;
    payload
        .validate(metadata)
        .map_err(|_| execution_payload_drift(run_id, &step.plan.id))?;
    if payload.flow_run_id != run_id || payload.flow_hook_id != hook_id {
        return Err(execution_payload_drift(run_id, &step.plan.id));
    }
    let (output, output_digest) = match payload.resolution {
        WorkflowExecutionResumeResolution::Rejected { reason } => {
            return Ok(ExecutionResolution::Failed(reason));
        }
        WorkflowExecutionResumeResolution::Completed {
            output,
            output_digest,
        } => {
            if let Some(error) = output.outcome.failure_message() {
                return Ok(ExecutionResolution::Failed(error));
            }
            (output, output_digest)
        }
    };
    let output = serde_json::to_value(&output)
        .map_err(|_| execution_payload_drift(run_id, &step.plan.id))?;
    let result = WorkflowLocalStepResult {
        step_id: step.plan.id.clone(),
        kind: WorkflowStepKind::Execution,
        output,
        output_digest,
        selected_handle: None,
    };
    result
        .validate(step)
        .map_err(|_| execution_payload_drift(run_id, &step.plan.id))?;
    Ok(ExecutionResolution::Succeeded(result))
}

fn execution_payload_drift(run_id: &str, step_id: &str) -> FlowError {
    FlowError::NonDeterministic {
        run_id: run_id.into(),
        reason: format!(
            "Workflow execution step {step_id:?} received an invalid authority-bound payload"
        ),
    }
}

pub(super) fn human_decision_result(
    run_id: &str,
    hook_id: &str,
    step: &ResolvedWorkflowRunStep,
    observed: &Value,
) -> Result<WorkflowLocalStepResult, FlowError> {
    let payload = serde_json::from_value::<FlowResumePayload>(observed.clone())
        .map_err(|_| human_decision_payload_drift(run_id, &step.plan.id))?;
    payload
        .validate()
        .map_err(|_| human_decision_payload_drift(run_id, &step.plan.id))?;
    if payload.workflow_run_id.to_string() != run_id
        || payload.flow_run_id != run_id
        || payload.flow_hook_id != hook_id
    {
        return Err(human_decision_payload_drift(run_id, &step.plan.id));
    }
    let output = serde_json::to_value(&payload.output)
        .map_err(|_| human_decision_payload_drift(run_id, &step.plan.id))?;
    let result = WorkflowLocalStepResult {
        step_id: step.plan.id.clone(),
        kind: WorkflowStepKind::HumanDecision,
        output,
        output_digest: payload.output_digest,
        selected_handle: None,
    };
    result
        .validate(step)
        .map_err(|_| human_decision_payload_drift(run_id, &step.plan.id))?;
    Ok(result)
}

fn human_decision_payload_drift(run_id: &str, step_id: &str) -> FlowError {
    FlowError::NonDeterministic {
        run_id: run_id.into(),
        reason: format!(
            "Workflow human-decision step {step_id:?} received an invalid authority-bound payload"
        ),
    }
}

fn incoming_edges(input: &WorkflowRunInput) -> BTreeMap<&str, Vec<&WorkflowEdgeSpec>> {
    let mut incoming = input
        .plan
        .steps
        .iter()
        .map(|step| (step.id.as_str(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for edge in &input.plan.edges {
        if let Some(edges) = incoming.get_mut(edge.target.as_str()) {
            edges.push(edge);
            edges.sort_by(|left, right| left.id.cmp(&right.id));
        }
    }
    incoming
}

fn dependency_state(
    step: &ResolvedWorkflowRunStep,
    incoming: &BTreeMap<&str, Vec<&WorkflowEdgeSpec>>,
    resolved: &BTreeMap<String, ResolvedState>,
) -> Result<Option<Option<BTreeMap<String, Value>>>, FlowError> {
    let edges = incoming.get(step.plan.id.as_str()).ok_or_else(|| {
        FlowError::InvalidWorkflow(format!(
            "Workflow step {:?} has no incoming-edge state",
            step.plan.id
        ))
    })?;
    if edges.is_empty() {
        return Ok(Some(Some(BTreeMap::new())));
    }
    let mut dependencies = BTreeMap::new();
    let mut active = false;
    for edge in edges {
        let Some(source) = resolved.get(&edge.source) else {
            return Ok(None);
        };
        let ResolvedState::Active(result) = source else {
            continue;
        };
        let edge_active = if result.kind == WorkflowStepKind::Branch {
            result.selected_handle.as_deref() == edge.source_handle.as_deref()
        } else {
            true
        };
        if edge_active {
            active = true;
            dependencies.insert(edge.source.clone(), result.output.clone());
        }
    }
    Ok(Some(active.then_some(dependencies)))
}

fn effective_input(dependencies: &BTreeMap<String, Value>, workflow_input: &Value) -> Value {
    match dependencies.len() {
        0 => workflow_input.clone(),
        1 => dependencies
            .first_key_value()
            .map(|(_, value)| value.clone())
            .unwrap_or(Value::Null),
        _ => Value::Object(
            dependencies
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        ),
    }
}

pub(super) fn inactive_step_ids(
    input: &WorkflowRunInput,
    completed: &BTreeMap<String, WorkflowLocalStepResult>,
) -> Result<BTreeSet<String>, String> {
    let steps = input.resolved_steps()?;
    let by_id = steps
        .iter()
        .map(|step| (step.plan.id.as_str(), step))
        .collect::<BTreeMap<_, _>>();
    let incoming = incoming_edges(input);
    let mut resolved = BTreeMap::<String, ResolvedState>::new();
    let mut inactive = BTreeSet::new();
    for planned in &input.plan.steps {
        let step = by_id
            .get(planned.id.as_str())
            .ok_or_else(|| format!("WorkflowRun lost step {:?}", planned.id))?;
        let dependency =
            dependency_state(step, &incoming, &resolved).map_err(|error| error.to_string())?;
        match dependency {
            Some(None) => {
                inactive.insert(planned.id.clone());
                resolved.insert(planned.id.clone(), ResolvedState::Inactive);
            }
            Some(Some(_)) => {
                if let Some(result) = completed.get(&planned.id) {
                    result.validate(step)?;
                    resolved.insert(planned.id.clone(), ResolvedState::Active(result.clone()));
                }
            }
            None => {}
        }
    }
    Ok(inactive)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{Sha256Digest, WorkflowRunId};
    use crate::modules::workflow::domain::{WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION};
    use crate::modules::workflow::test_support::{digest, timestamp, workflow_run_input};
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

    fn invocation(input: &WorkflowRunInput, history: Vec<FlowEventEnvelope>) -> WorkflowInvocation {
        WorkflowInvocation {
            run_id: input.workflow_run_id.to_string(),
            spec: WorkflowSpec::rust_embedded(
                WORKFLOW_RUN_FLOW_NAME,
                WORKFLOW_RUN_FLOW_VERSION,
                "cloud",
                "workflow_run",
            ),
            input: serde_json::to_value(input).expect("WorkflowRun input JSON"),
            history,
        }
    }

    fn envelope(
        input: &WorkflowRunInput,
        sequence: u64,
        timestamp: chrono::DateTime<chrono::Utc>,
        event: FlowEvent,
    ) -> FlowEventEnvelope {
        FlowEventEnvelope {
            run_id: input.workflow_run_id.to_string(),
            sequence,
            event_id: Uuid::now_v7(),
            timestamp,
            event,
        }
    }
}
