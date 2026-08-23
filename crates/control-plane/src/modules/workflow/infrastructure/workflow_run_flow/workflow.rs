pub(super) use super::workflow_application_variables::{
    application_variable_snapshot_result, application_variable_write_result,
};
use super::{
    decode_input, WorkflowLocalStepInput, WorkflowLocalStepResult, WORKFLOW_RUN_STEP_NAME,
};
use crate::modules::workflow::domain::{
    flow_step_id, FlowResumePayload, ResolvedWorkflowRunStep,
    WorkflowApplicationAnswerFailureResumePayload, WorkflowApplicationAnswerHookMetadata,
    WorkflowApplicationAnswerResumePayload, WorkflowApplicationVariableSnapshotHookMetadata,
    WorkflowApplicationVariableWriteFailureResumePayload,
    WorkflowApplicationVariableWriteHookMetadata, WorkflowEdgeSpec, WorkflowExecutionHookMetadata,
    WorkflowExecutionResumePayload, WorkflowExecutionResumeResolution, WorkflowExecutionStepOutput,
    WorkflowHumanDecisionHookMetadata, WorkflowRunInput, WorkflowStepDefaultOutputEvidence,
    WorkflowStepFailureClassification, WorkflowStepFailureOutput, WorkflowStepKind,
    WORKFLOW_APPLICATION_ANSWER_FAILURE_RESUME_SCHEMA,
    WORKFLOW_APPLICATION_ANSWER_FAILURE_RESUME_SCHEMA_V2,
    WORKFLOW_APPLICATION_ANSWER_RESUME_SCHEMA, WORKFLOW_APPLICATION_ANSWER_RESUME_SCHEMA_V2,
    WORKFLOW_APPLICATION_VARIABLE_WRITE_FAILURE_RESUME_SCHEMA,
    WORKFLOW_APPLICATION_VARIABLE_WRITE_RESUME_SCHEMA, WORKFLOW_RUN_INPUT_SCHEMA_V14,
    WORKFLOW_RUN_INPUT_SCHEMA_V15,
};
use a3s_flow::{FlowError, RetryPolicy, RuntimeCommand, WorkflowInvocation};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
enum ResolvedState {
    Active(Box<WorkflowLocalStepResult>),
    Inactive,
}

pub(super) enum ApplicationVariableWriteResolution {
    Completed(Box<WorkflowLocalStepResult>),
    Failed {
        result: Box<WorkflowLocalStepResult>,
        message: String,
    },
}

pub(super) enum ApplicationAnswerResolution {
    Completed(Box<WorkflowLocalStepResult>),
    Failed {
        result: Box<WorkflowLocalStepResult>,
        message: String,
    },
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
        let legacy_input = effective_input(&dependencies, &input.goal_input);
        let all_steps = resolved
            .iter()
            .filter_map(|(id, state)| match state {
                ResolvedState::Active(result) => Some((id.clone(), result.output.clone())),
                ResolvedState::Inactive => None,
            })
            .collect::<BTreeMap<_, _>>();
        let composite_results = resolved
            .iter()
            .filter_map(|(id, state)| match state {
                ResolvedState::Active(result) => result
                    .composite_region_result
                    .clone()
                    .map(|region| (id.clone(), region)),
                ResolvedState::Inactive => None,
            })
            .collect::<BTreeMap<_, _>>();
        let application_snapshot = if input
            .application_projection
            .as_ref()
            .is_some_and(|projection| projection.is_variable_step(&step.plan.id))
        {
            let metadata =
                WorkflowApplicationVariableSnapshotHookMetadata::from_run_step(&input, step)
                    .map_err(FlowError::InvalidWorkflow)?;
            let hook_id = metadata.flow_hook_id();
            if context.hook_disposed(&hook_id) {
                return Ok(context.fail(format!(
                    "Workflow Application variable snapshot hook for step {:?} was disposed",
                    step.plan.id
                )));
            }
            match context.hook_payload(&hook_id) {
                Some(payload) => Some(application_variable_snapshot_result(
                    &invocation.run_id,
                    &hook_id,
                    step,
                    &metadata,
                    payload,
                )?),
                None => {
                    return Ok(context.create_hook(
                        hook_id,
                        metadata.flow_hook_token(),
                        serde_json::to_value(metadata)?,
                    ))
                }
            }
        } else {
            None
        };
        let (effective_input, typed_projection_authoritative) =
            if step.plan.kind == WorkflowStepKind::Subworkflow {
                (legacy_input, false)
            } else {
                let projection = super::variables::effective_input(
                    &input,
                    &step.plan.id,
                    legacy_input,
                    &all_steps,
                    &composite_results,
                    application_snapshot.as_ref(),
                )
                .map_err(FlowError::InvalidWorkflow)?;
                (projection.input, projection.authoritative)
            };
        if step.plan.kind == WorkflowStepKind::Subworkflow {
            let durable_step_id = flow_step_id(&step.plan.id);
            if let Some(error) = context.step_failed(&durable_step_id) {
                return Ok(context.fail(format!(
                    "Workflow composite step {:?} failed: {error}",
                    step.plan.id
                )));
            }
            if let Some(value) = context.step_output(&durable_step_id) {
                let result = serde_json::from_value::<WorkflowLocalStepResult>(value.clone())?;
                super::composite::validate_result_authority(&input, step, &result).map_err(
                    |error| FlowError::NonDeterministic {
                        run_id: invocation.run_id.clone(),
                        reason: format!(
                            "Workflow composite step {:?} replay result drifted: {error}",
                            step.plan.id
                        ),
                    },
                )?;
                resolved.insert(
                    step.plan.id.clone(),
                    ResolvedState::Active(Box::new(result)),
                );
                continue;
            }
            let resolution = match super::composite::resolve_step(
                &input,
                step,
                effective_input.clone(),
                &all_steps,
                &composite_results,
                &context,
            ) {
                Ok(resolution) => resolution,
                Err(super::composite::CompositeStepError::Invalid(error)) => {
                    return Err(FlowError::InvalidWorkflow(error))
                }
                Err(super::composite::CompositeStepError::NonDeterministic(reason)) => {
                    return Err(FlowError::NonDeterministic {
                        run_id: invocation.run_id.clone(),
                        reason,
                    })
                }
            };
            match resolution {
                super::composite::CompositeStepResolution::Await(metadata) => {
                    return Ok(context.create_hook(
                        metadata.flow_hook_id(),
                        metadata.flow_hook_token(),
                        serde_json::to_value(metadata)?,
                    ));
                }
                super::composite::CompositeStepResolution::Complete(region) => {
                    let step_input = WorkflowLocalStepInput {
                        runtime_contract_revision: input.runtime_contract_revision.clone(),
                        typed_projection_authoritative: false,
                        step: (*step).clone(),
                        workflow_input: input.goal_input.clone(),
                        effective_input,
                        dependencies,
                        steps: all_steps,
                        composite_region_result: Some(region),
                    };
                    ready.push(context.step(
                        durable_step_id,
                        WORKFLOW_RUN_STEP_NAME,
                        serde_json::to_value(step_input)?,
                    ));
                    continue;
                }
                super::composite::CompositeStepResolution::Failed(error) => {
                    return Ok(context.fail(format!(
                        "Workflow composite step {:?} failed: {error}",
                        step.plan.id
                    )));
                }
            }
        }
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
                resolved.insert(
                    step.plan.id.clone(),
                    ResolvedState::Active(Box::new(result)),
                );
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
                match execution_result(
                    &invocation.run_id,
                    &hook_id,
                    &input,
                    step,
                    &metadata,
                    payload,
                )? {
                    ExecutionResolution::Succeeded(result) => {
                        resolved.insert(step.plan.id.clone(), ResolvedState::Active(result));
                        continue;
                    }
                    ExecutionResolution::Failed {
                        error: _,
                        routed: Some(result),
                    } => {
                        resolved.insert(step.plan.id.clone(), ResolvedState::Active(result));
                        continue;
                    }
                    ExecutionResolution::Failed {
                        error,
                        routed: None,
                    } => {
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
        if input
            .application_projection
            .as_ref()
            .is_some_and(|projection| projection.is_variable_assignment_step(&step.plan.id))
        {
            super::execution::validate_data_schema(
                &step.input_schema,
                &effective_input,
                "Workflow Application variable assignment step input",
            )
            .map_err(FlowError::InvalidWorkflow)?;
            let application_snapshot = application_snapshot.as_ref().ok_or_else(|| {
                FlowError::InvalidWorkflow(format!(
                    "Workflow Application variable assignment step {:?} lost its snapshot",
                    step.plan.id
                ))
            })?;
            let values = super::variables::application_assignment_values(
                &input,
                &step.plan.id,
                &all_steps,
                &composite_results,
                application_snapshot,
            )
            .map_err(FlowError::InvalidWorkflow)?;
            let metadata = WorkflowApplicationVariableWriteHookMetadata::from_run_step(
                &input,
                step,
                application_snapshot,
                &values,
            )
            .map_err(FlowError::InvalidWorkflow)?;
            let hook_id = metadata.flow_hook_id();
            if context.hook_disposed(&hook_id) {
                return Ok(context.fail(format!(
                    "Workflow Application variable write hook for step {:?} was disposed",
                    step.plan.id
                )));
            }
            if let Some(payload) = context.hook_payload(&hook_id) {
                let resolution = application_variable_write_resolution(
                    &invocation.run_id,
                    &hook_id,
                    &input,
                    step,
                    &metadata,
                    &values,
                    payload,
                )?;
                let result = match resolution {
                    ApplicationVariableWriteResolution::Completed(result) => *result,
                    ApplicationVariableWriteResolution::Failed { result, .. } => *result,
                };
                resolved.insert(
                    step.plan.id.clone(),
                    ResolvedState::Active(Box::new(result)),
                );
                continue;
            }
            return Ok(context.create_hook(
                hook_id,
                metadata.flow_hook_token(),
                serde_json::to_value(metadata)?,
            ));
        }
        if step.plan.kind == WorkflowStepKind::Service {
            super::execution::validate_data_schema(
                &step.input_schema,
                &effective_input,
                "Workflow Connector step input",
            )
            .map_err(FlowError::InvalidWorkflow)?;
            let resolution = super::connector::resolve_step(
                &invocation.run_id,
                &input,
                step,
                effective_input.clone(),
                &context,
            )
            .map_err(|error| super::connector::flow_error(&invocation.run_id, error))?;
            match resolution {
                super::connector::ConnectorStepResolution::Await(metadata) => {
                    return Ok(context.create_hook(
                        metadata.flow_hook_id(),
                        metadata.flow_hook_token(),
                        serde_json::to_value(metadata)?,
                    ));
                }
                super::connector::ConnectorStepResolution::Wait { wait_id, resume_at } => {
                    return Ok(context.wait_until(wait_id, resume_at));
                }
                super::connector::ConnectorStepResolution::Complete(result) => {
                    resolved.insert(step.plan.id.clone(), ResolvedState::Active(result));
                    continue;
                }
                super::connector::ConnectorStepResolution::Consume(response) => {
                    let durable_step_id = flow_step_id(&step.plan.id);
                    if let Some(error) = context.step_failed(&durable_step_id) {
                        if let Some(result) = connector_failure_route_result(
                            &invocation.run_id,
                            &input,
                            step,
                            WorkflowStepFailureClassification::ProviderResponseInvalid,
                            error.to_owned(),
                        )? {
                            resolved.insert(step.plan.id.clone(), ResolvedState::Active(result));
                            continue;
                        }
                        return Ok(context.fail(format!(
                            "Workflow Connector response step {:?} failed: {error}",
                            step.plan.id
                        )));
                    }
                    if let Some(value) = context.step_output(&durable_step_id) {
                        let result =
                            serde_json::from_value::<WorkflowLocalStepResult>(value.clone())?;
                        response.validate_result(&result).map_err(|reason| {
                            FlowError::NonDeterministic {
                                run_id: invocation.run_id.clone(),
                                reason: format!(
                                    "Workflow Connector response step {:?} replay result drifted: {reason}",
                                    step.plan.id
                                ),
                            }
                        })?;
                        resolved.insert(
                            step.plan.id.clone(),
                            ResolvedState::Active(Box::new(result)),
                        );
                        continue;
                    }
                    let retry = if failure_route_handle(&input, step)?.is_some() {
                        RetryPolicy::none().continue_workflow_on_failure()
                    } else {
                        RetryPolicy::none()
                    };
                    return Ok(context.schedule_steps(vec![context.step_with_retry(
                        durable_step_id,
                        super::connector_response::WORKFLOW_CONNECTOR_RESPONSE_STEP_NAME,
                        serde_json::to_value(*response)?,
                        retry,
                    )]));
                }
                super::connector::ConnectorStepResolution::Failed(failure) => {
                    if let Some(result) = connector_failure_route_result(
                        &invocation.run_id,
                        &input,
                        step,
                        failure.classification,
                        failure.message.clone(),
                    )? {
                        resolved.insert(step.plan.id.clone(), ResolvedState::Active(result));
                        continue;
                    }
                    return Ok(context.fail(format!(
                        "Workflow Connector step {:?} failed: {error}",
                        step.plan.id,
                        error = failure.message
                    )));
                }
            }
        }
        if input
            .application_projection
            .as_ref()
            .is_some_and(|projection| projection.is_answer_step(&step.plan.id))
        {
            let step_input = WorkflowLocalStepInput {
                runtime_contract_revision: input.runtime_contract_revision.clone(),
                typed_projection_authoritative,
                step: (*step).clone(),
                workflow_input: input.goal_input.clone(),
                effective_input,
                dependencies,
                steps: all_steps,
                composite_region_result: None,
            };
            let prepared = super::execution::execute_local_step(&step_input)
                .map_err(FlowError::InvalidWorkflow)?;
            let metadata = WorkflowApplicationAnswerHookMetadata::from_run_step(
                &input,
                step,
                prepared.output.clone(),
            )
            .map_err(FlowError::InvalidWorkflow)?;
            if metadata.content_digest != prepared.output_digest {
                return Err(FlowError::InvalidWorkflow(format!(
                    "Workflow Application Answer step {:?} prepared a drifted output digest",
                    step.plan.id
                )));
            }
            let hook_id = metadata.flow_hook_id();
            if context.hook_disposed(&hook_id) {
                return Ok(context.fail(format!(
                    "Workflow Application Answer hook for step {:?} was disposed",
                    step.plan.id
                )));
            }
            if let Some(payload) = context.hook_payload(&hook_id) {
                let resolution = application_answer_resolution(
                    &invocation.run_id,
                    &hook_id,
                    &input,
                    step,
                    &metadata,
                    payload,
                )?;
                let result = match resolution {
                    ApplicationAnswerResolution::Completed(result) => *result,
                    ApplicationAnswerResolution::Failed { result, .. } => *result,
                };
                resolved.insert(
                    step.plan.id.clone(),
                    ResolvedState::Active(Box::new(result)),
                );
                continue;
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
            super::composite::validate_result_authority(&input, step, &result).map_err(
                |error| FlowError::NonDeterministic {
                    run_id: invocation.run_id.clone(),
                    reason: format!(
                        "Workflow step {:?} composite result drifted: {error}",
                        step.plan.id
                    ),
                },
            )?;
            resolved.insert(
                step.plan.id.clone(),
                ResolvedState::Active(Box::new(result)),
            );
            continue;
        }
        let step_input = WorkflowLocalStepInput {
            runtime_contract_revision: input.runtime_contract_revision.clone(),
            typed_projection_authoritative,
            step: (*step).clone(),
            workflow_input: input.goal_input.clone(),
            effective_input,
            dependencies,
            steps: all_steps,
            composite_region_result: None,
        };
        ready.push(context.step(
            durable_step_id,
            WORKFLOW_RUN_STEP_NAME,
            serde_json::to_value(step_input)?,
        ));
    }

    if !ready.is_empty() {
        return Ok(context.schedule_steps(ready));
    }
    match resolved_workflow_output(&input, &resolved) {
        Ok(Some(output)) => Ok(context.complete(output)),
        Ok(None) => Ok(
            context.fail("WorkflowRun graph stalled before all reachable output sinks resolved")
        ),
        Err(error) => Ok(context.fail(error)),
    }
}

pub(super) enum ExecutionResolution {
    Succeeded(Box<WorkflowLocalStepResult>),
    Failed {
        error: String,
        routed: Option<Box<WorkflowLocalStepResult>>,
    },
}

pub(super) fn execution_result(
    run_id: &str,
    hook_id: &str,
    input: &WorkflowRunInput,
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
            return execution_failure_resolution(run_id, input, step, reason, None);
        }
        WorkflowExecutionResumeResolution::Completed {
            output,
            output_digest,
        } => {
            if let Some(error) = output.outcome.failure_message() {
                return execution_failure_resolution(run_id, input, step, error, Some(output));
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
        composite_region_result: None,
        default_output_evidence: None,
    };
    result
        .validate(step)
        .map_err(|_| execution_payload_drift(run_id, &step.plan.id))?;
    Ok(ExecutionResolution::Succeeded(Box::new(result)))
}

fn execution_failure_resolution(
    run_id: &str,
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    error: String,
    detail: Option<WorkflowExecutionStepOutput>,
) -> Result<ExecutionResolution, FlowError> {
    if step.plan.default_output.is_some() {
        let failure = match detail {
            Some(output) => WorkflowStepFailureOutput::observe_execution(step, output),
            None => WorkflowStepFailureOutput::observe_dispatch_rejected(step, error),
        }
        .map_err(|_| execution_payload_drift(run_id, &step.plan.id))?;
        let evidence = WorkflowStepDefaultOutputEvidence::new(step, failure)
            .map_err(|_| execution_payload_drift(run_id, &step.plan.id))?;
        let material = step
            .policy
            .as_ref()
            .and_then(|policy| policy.default_output.as_ref())
            .ok_or_else(|| execution_payload_drift(run_id, &step.plan.id))?;
        let result = WorkflowLocalStepResult {
            step_id: step.plan.id.clone(),
            kind: WorkflowStepKind::Execution,
            output: material.value.clone(),
            output_digest: material.digest.clone(),
            selected_handle: None,
            composite_region_result: None,
            default_output_evidence: Some(evidence),
        };
        result
            .validate(step)
            .map_err(|_| execution_payload_drift(run_id, &step.plan.id))?;
        return Ok(ExecutionResolution::Succeeded(Box::new(result)));
    }
    if failure_route_handle(input, step)?.is_none() {
        return Ok(ExecutionResolution::Failed {
            error,
            routed: None,
        });
    }
    let failure = match detail {
        Some(output) => WorkflowStepFailureOutput::from_execution(step, output),
        None => WorkflowStepFailureOutput::dispatch_rejected(step, error.clone()),
    }
    .map_err(|_| execution_payload_drift(run_id, &step.plan.id))?;
    let routed = failure_route_result(run_id, input, step, failure)?;
    Ok(ExecutionResolution::Failed { error, routed })
}

pub(super) fn connector_failure_route_result(
    run_id: &str,
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    classification: WorkflowStepFailureClassification,
    message: String,
) -> Result<Option<Box<WorkflowLocalStepResult>>, FlowError> {
    if failure_route_handle(input, step)?.is_none() {
        return Ok(None);
    }
    let failure =
        WorkflowStepFailureOutput::provider(step, classification, message).map_err(|_| {
            FlowError::NonDeterministic {
                run_id: run_id.into(),
                reason: format!(
                "Workflow Connector step {:?} could not materialize its descriptor-bound failure",
                step.plan.id
            ),
            }
        })?;
    failure_route_result(run_id, input, step, failure)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn application_variable_write_resolution(
    run_id: &str,
    hook_id: &str,
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    metadata: &WorkflowApplicationVariableWriteHookMetadata,
    values: &Value,
    observed: &Value,
) -> Result<ApplicationVariableWriteResolution, FlowError> {
    match observed.get("schema").and_then(Value::as_str) {
        Some(WORKFLOW_APPLICATION_VARIABLE_WRITE_RESUME_SCHEMA) => {
            application_variable_write_result(run_id, hook_id, step, metadata, values, observed)
                .map(Box::new)
                .map(ApplicationVariableWriteResolution::Completed)
        }
        Some(WORKFLOW_APPLICATION_VARIABLE_WRITE_FAILURE_RESUME_SCHEMA)
            if matches!(
                input.schema.as_str(),
                WORKFLOW_RUN_INPUT_SCHEMA_V14 | WORKFLOW_RUN_INPUT_SCHEMA_V15
            ) =>
        {
            let payload = serde_json::from_value::<
                WorkflowApplicationVariableWriteFailureResumePayload,
            >(observed.clone())
            .map_err(|_| application_variable_failure_payload_drift(run_id, &step.plan.id))?;
            payload
                .validate(metadata)
                .map_err(|_| application_variable_failure_payload_drift(run_id, &step.plan.id))?;
            if payload.flow_run_id != run_id || payload.flow_hook_id != hook_id {
                return Err(application_variable_failure_payload_drift(
                    run_id,
                    &step.plan.id,
                ));
            }
            let failure =
                WorkflowStepFailureOutput::application_variable(step, payload.classification)
                    .map_err(|_| {
                        application_variable_failure_payload_drift(run_id, &step.plan.id)
                    })?;
            let message = failure.message.clone();
            let result = failure_route_result(run_id, input, step, failure)?
                .ok_or_else(|| application_variable_failure_payload_drift(run_id, &step.plan.id))?;
            Ok(ApplicationVariableWriteResolution::Failed { result, message })
        }
        _ => Err(application_variable_failure_payload_drift(
            run_id,
            &step.plan.id,
        )),
    }
}

fn failure_route_result(
    run_id: &str,
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    failure: WorkflowStepFailureOutput,
) -> Result<Option<Box<WorkflowLocalStepResult>>, FlowError> {
    let Some(handle) = failure_route_handle(input, step)? else {
        return Ok(None);
    };
    let output =
        serde_json::to_value(failure).map_err(|_| failure_payload_drift(run_id, &step.plan.id))?;
    let output_digest = super::execution::value_digest(&output, "Workflow step failure output")
        .map_err(|_| failure_payload_drift(run_id, &step.plan.id))?;
    let result = WorkflowLocalStepResult {
        step_id: step.plan.id.clone(),
        kind: step.plan.kind,
        output,
        output_digest,
        selected_handle: Some(handle),
        composite_region_result: None,
        default_output_evidence: None,
    };
    result
        .validate(step)
        .map_err(|_| failure_payload_drift(run_id, &step.plan.id))?;
    Ok(Some(Box::new(result)))
}

pub(super) fn failure_route_handle(
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
) -> Result<Option<String>, FlowError> {
    let handles = input
        .plan
        .edges
        .iter()
        .filter(|edge| edge.source == step.plan.id)
        .filter_map(|edge| edge.source_handle.as_ref())
        .collect::<Vec<_>>();
    match handles.as_slice() {
        [] => Ok(None),
        [handle] => Ok(Some((*handle).clone())),
        _ => Err(FlowError::InvalidWorkflow(format!(
            "Workflow step {:?} has more than one failure route",
            step.plan.id
        ))),
    }
}

fn failure_payload_drift(run_id: &str, step_id: &str) -> FlowError {
    FlowError::NonDeterministic {
        run_id: run_id.into(),
        reason: format!(
            "Workflow step {step_id:?} could not restore its descriptor-bound failure value"
        ),
    }
}

fn application_variable_failure_payload_drift(run_id: &str, step_id: &str) -> FlowError {
    FlowError::NonDeterministic {
        run_id: run_id.into(),
        reason: format!(
            "Workflow Application variable step {step_id:?} received invalid failure evidence"
        ),
    }
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
        composite_region_result: None,
        default_output_evidence: None,
    };
    result
        .validate(step)
        .map_err(|_| human_decision_payload_drift(run_id, &step.plan.id))?;
    Ok(result)
}

pub(super) fn application_answer_result(
    run_id: &str,
    hook_id: &str,
    step: &ResolvedWorkflowRunStep,
    metadata: &WorkflowApplicationAnswerHookMetadata,
    observed: &Value,
) -> Result<WorkflowLocalStepResult, FlowError> {
    let payload =
        serde_json::from_value::<WorkflowApplicationAnswerResumePayload>(observed.clone())
            .map_err(|_| application_answer_payload_drift(run_id, &step.plan.id))?;
    payload
        .validate(metadata)
        .map_err(|_| application_answer_payload_drift(run_id, &step.plan.id))?;
    if payload.flow_run_id != run_id || payload.flow_hook_id != hook_id {
        return Err(application_answer_payload_drift(run_id, &step.plan.id));
    }
    let result = WorkflowLocalStepResult {
        step_id: step.plan.id.clone(),
        kind: WorkflowStepKind::Output,
        output: metadata.content.clone(),
        output_digest: metadata.content_digest.clone(),
        selected_handle: None,
        composite_region_result: None,
        default_output_evidence: None,
    };
    result
        .validate(step)
        .map_err(|_| application_answer_payload_drift(run_id, &step.plan.id))?;
    Ok(result)
}

pub(super) fn application_answer_resolution(
    run_id: &str,
    hook_id: &str,
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    metadata: &WorkflowApplicationAnswerHookMetadata,
    observed: &Value,
) -> Result<ApplicationAnswerResolution, FlowError> {
    match observed.get("schema").and_then(Value::as_str) {
        Some(WORKFLOW_APPLICATION_ANSWER_RESUME_SCHEMA)
        | Some(WORKFLOW_APPLICATION_ANSWER_RESUME_SCHEMA_V2) => {
            application_answer_result(run_id, hook_id, step, metadata, observed)
                .map(Box::new)
                .map(ApplicationAnswerResolution::Completed)
        }
        Some(WORKFLOW_APPLICATION_ANSWER_FAILURE_RESUME_SCHEMA)
        | Some(WORKFLOW_APPLICATION_ANSWER_FAILURE_RESUME_SCHEMA_V2)
            if input.schema == WORKFLOW_RUN_INPUT_SCHEMA_V15 =>
        {
            let payload = serde_json::from_value::<WorkflowApplicationAnswerFailureResumePayload>(
                observed.clone(),
            )
            .map_err(|_| application_answer_payload_drift(run_id, &step.plan.id))?;
            payload
                .validate(metadata)
                .map_err(|_| application_answer_payload_drift(run_id, &step.plan.id))?;
            if payload.flow_run_id != run_id || payload.flow_hook_id != hook_id {
                return Err(application_answer_payload_drift(run_id, &step.plan.id));
            }
            let failure =
                WorkflowStepFailureOutput::application_answer(step, payload.classification)
                    .map_err(|_| application_answer_payload_drift(run_id, &step.plan.id))?;
            let message = failure.message.clone();
            let result = failure_route_result(run_id, input, step, failure)?
                .ok_or_else(|| application_answer_payload_drift(run_id, &step.plan.id))?;
            Ok(ApplicationAnswerResolution::Failed { result, message })
        }
        _ => Err(application_answer_payload_drift(run_id, &step.plan.id)),
    }
}

fn application_answer_payload_drift(run_id: &str, step_id: &str) -> FlowError {
    FlowError::NonDeterministic {
        run_id: run_id.into(),
        reason: format!(
            "Workflow Application Answer step {step_id:?} received an invalid authority-bound payload"
        ),
    }
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
        let edge_active = result.selected_handle.as_deref() == edge.source_handle.as_deref();
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

fn resolved_workflow_output(
    input: &WorkflowRunInput,
    resolved: &BTreeMap<String, ResolvedState>,
) -> Result<Option<Value>, String> {
    let outputs = input
        .plan
        .steps
        .iter()
        .filter(|step| step.kind == WorkflowStepKind::Output)
        .collect::<Vec<_>>();
    if let Some(projection) = input
        .application_projection
        .as_ref()
        .filter(|projection| !projection.answer_step_ids.is_empty())
    {
        if outputs
            .iter()
            .any(|output| !resolved.contains_key(&output.id))
        {
            return Ok(None);
        }
        let final_output = resolved
            .get(&projection.final_output_step_id)
            .ok_or_else(|| "WorkflowRun lost its projected final Output step".to_owned())?;
        return match final_output {
            ResolvedState::Active(result) => {
                super::execution::value_digest(&result.output, "WorkflowRun aggregate output")?;
                Ok(Some(result.output.clone()))
            }
            ResolvedState::Inactive => {
                Err("WorkflowRun resolved no reachable final Output step".into())
            }
        };
    }
    let mut active = BTreeMap::new();
    for output in &outputs {
        match resolved.get(&output.id) {
            Some(ResolvedState::Active(result)) => {
                active.insert(output.id.clone(), result.output.clone());
            }
            Some(ResolvedState::Inactive) => {}
            None => return Ok(None),
        }
    }
    if active.is_empty() {
        return Err("WorkflowRun resolved no reachable output sink".into());
    }
    let output = if outputs.len() == 1 {
        active
            .into_values()
            .next()
            .ok_or_else(|| "WorkflowRun lost its reachable output".to_owned())?
    } else {
        Value::Object(active.into_iter().collect())
    };
    super::execution::value_digest(&output, "WorkflowRun aggregate output")?;
    Ok(Some(output))
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
                    resolved.insert(
                        planned.id.clone(),
                        ResolvedState::Active(Box::new(result.clone())),
                    );
                }
            }
            None => {}
        }
    }
    Ok(inactive)
}

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;
