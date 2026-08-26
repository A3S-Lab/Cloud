use super::{connector, WorkflowLocalStepResult};
use crate::modules::workflow::domain::{
    flow_step_id, ResolvedWorkflowRunStep, WorkflowConnectorHookMetadata,
    WorkflowConnectorInvocationPurpose, WorkflowRunInput, WorkflowStepFailureClassification,
    WORKFLOW_RUN_INPUT_SCHEMA_V23, WORKFLOW_RUN_INPUT_SCHEMA_V24, WORKFLOW_RUN_INPUT_SCHEMA_V25,
};
use a3s_flow::{FlowError, FlowEvent, RetryPolicy, RuntimeCommand, WorkflowContext};

pub(super) fn cancellation_source_response_step_id(step_id: &str) -> String {
    format!("workflow-cancellation-source-response:{step_id}")
}

enum CompletedConnectorOutput {
    Unavailable,
    Complete(serde_json::Value),
    Materialize(Box<super::connector_response::WorkflowConnectorResponseStepInput>),
    Failed(String),
}

enum OrdinaryConnectorEffect {
    Unavailable,
    Accepted,
    Failed(String),
}

pub(super) fn resolve_cancellation(
    run_id: &str,
    input: &WorkflowRunInput,
    context: &WorkflowContext<'_>,
) -> Result<RuntimeCommand, FlowError> {
    if !matches!(
        input.schema.as_str(),
        WORKFLOW_RUN_INPUT_SCHEMA_V23
            | WORKFLOW_RUN_INPUT_SCHEMA_V24
            | WORKFLOW_RUN_INPUT_SCHEMA_V25
    ) {
        return Ok(context.cancel());
    }
    let resolved = input.resolved_steps().map_err(FlowError::InvalidWorkflow)?;
    for source in resolved.iter().rev() {
        let Some(compensation) = source
            .policy
            .as_ref()
            .and_then(|policy| policy.cancellation_compensation.as_ref())
        else {
            continue;
        };
        let target = resolved
            .iter()
            .find(|step| step.plan.id == compensation.step_id)
            .ok_or_else(|| {
                FlowError::InvalidWorkflow(format!(
                    "Workflow cancellation compensation for {:?} lost target {:?}",
                    source.plan.id, compensation.step_id
                ))
            })?;

        match ordinary_connector_effect_observed(run_id, input, target, context)? {
            OrdinaryConnectorEffect::Unavailable => {}
            OrdinaryConnectorEffect::Accepted => continue,
            OrdinaryConnectorEffect::Failed(error) => return Ok(context.fail(error)),
        }
        let source_output = match completed_connector_output(run_id, input, source, context)? {
            CompletedConnectorOutput::Unavailable => continue,
            CompletedConnectorOutput::Complete(output) => output,
            CompletedConnectorOutput::Materialize(response) => {
                return Ok(context.schedule_steps(vec![context.step_with_retry(
                    cancellation_source_response_step_id(&source.plan.id),
                    super::connector_response::WORKFLOW_CONNECTOR_RESPONSE_STEP_NAME,
                    serde_json::to_value(*response)?,
                    RetryPolicy::none(),
                )]));
            }
            CompletedConnectorOutput::Failed(error) => return Ok(context.fail(error)),
        };
        super::execution::validate_data_schema(
            &target.input_schema,
            &source_output,
            "Workflow cancellation-compensation input",
        )
        .map_err(FlowError::InvalidWorkflow)?;
        let resolution = connector::resolve_cancellation_compensation(
            run_id,
            input,
            source,
            target,
            source_output,
            context,
        )
        .map_err(|error| connector::flow_error(run_id, error))?;
        match resolution {
            connector::ConnectorStepResolution::Await(metadata) => {
                return Ok(context.create_hook(
                    metadata.flow_hook_id(),
                    metadata.flow_hook_token(),
                    serde_json::to_value(metadata)?,
                ));
            }
            connector::ConnectorStepResolution::Wait { wait_id, resume_at } => {
                return Ok(context.wait_until(wait_id, resume_at));
            }
            connector::ConnectorStepResolution::Complete(_) => continue,
            connector::ConnectorStepResolution::Consume(response) => {
                let durable_step_id = flow_step_id(&target.plan.id);
                if let Some(error) = context.step_failed(&durable_step_id) {
                    return Ok(context.fail(format!(
                        "Workflow cancellation compensation {:?} response failed: {error}",
                        target.plan.id
                    )));
                }
                if let Some(value) = context.step_output(&durable_step_id) {
                    let result = serde_json::from_value::<WorkflowLocalStepResult>(value.clone())?;
                    response.validate_result(&result).map_err(|reason| {
                        FlowError::NonDeterministic {
                            run_id: run_id.into(),
                            reason: format!(
                                "Workflow cancellation compensation {:?} response drifted: {reason}",
                                target.plan.id
                            ),
                        }
                    })?;
                    continue;
                }
                return Ok(context.schedule_steps(vec![context.step_with_retry(
                    durable_step_id,
                    super::connector_response::WORKFLOW_CONNECTOR_RESPONSE_STEP_NAME,
                    serde_json::to_value(*response)?,
                    RetryPolicy::none(),
                )]));
            }
            connector::ConnectorStepResolution::Failed(failure) => {
                return Ok(context.fail(format!(
                    "Workflow cancellation compensation {:?} failed: {}",
                    target.plan.id, failure.message
                )));
            }
        }
    }
    Ok(context.cancel())
}

fn completed_connector_output(
    run_id: &str,
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    context: &WorkflowContext<'_>,
) -> Result<CompletedConnectorOutput, FlowError> {
    let Some(effective_input) = normal_effective_input(input, step, context)? else {
        return Ok(CompletedConnectorOutput::Unavailable);
    };
    let resolution = connector::resolve_step(run_id, input, step, effective_input, context)
        .map_err(|error| connector::flow_error(run_id, error))?;
    match resolution {
        connector::ConnectorStepResolution::Complete(result) => {
            Ok(CompletedConnectorOutput::Complete(result.output))
        }
        connector::ConnectorStepResolution::Consume(response) => {
            let durable_step_id = flow_step_id(&step.plan.id);
            if let Some(value) = context.step_output(&durable_step_id) {
                let result = serde_json::from_value::<WorkflowLocalStepResult>(value.clone())?;
                response
                    .validate_result(&result)
                    .map_err(FlowError::InvalidWorkflow)?;
                return Ok(CompletedConnectorOutput::Complete(result.output));
            }
            let cleanup_step_id = cancellation_source_response_step_id(&step.plan.id);
            if let Some(error) = context.step_failed(&cleanup_step_id) {
                return Ok(CompletedConnectorOutput::Failed(format!(
                    "Workflow cancellation source {:?} response failed: {error}",
                    step.plan.id
                )));
            }
            if let Some(value) = context.step_output(&cleanup_step_id) {
                let result = serde_json::from_value::<WorkflowLocalStepResult>(value.clone())?;
                response
                    .validate_result(&result)
                    .map_err(FlowError::InvalidWorkflow)?;
                return Ok(CompletedConnectorOutput::Complete(result.output));
            }
            Ok(CompletedConnectorOutput::Materialize(response))
        }
        connector::ConnectorStepResolution::Failed(failure)
            if failure.classification
                == WorkflowStepFailureClassification::ProviderIndeterminate =>
        {
            Ok(CompletedConnectorOutput::Failed(format!(
                "Workflow Connector {:?} has an indeterminate side effect and cannot be cancelled safely",
                step.plan.id
            )))
        }
        connector::ConnectorStepResolution::Await(_)
        | connector::ConnectorStepResolution::Wait { .. }
        | connector::ConnectorStepResolution::Failed(_) => {
            Ok(CompletedConnectorOutput::Unavailable)
        }
    }
}

fn ordinary_connector_effect_observed(
    run_id: &str,
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    context: &WorkflowContext<'_>,
) -> Result<OrdinaryConnectorEffect, FlowError> {
    let Some(effective_input) = normal_effective_input(input, step, context)? else {
        return Ok(OrdinaryConnectorEffect::Unavailable);
    };
    let resolution = connector::resolve_step(run_id, input, step, effective_input, context)
        .map_err(|error| connector::flow_error(run_id, error))?;
    match resolution {
        connector::ConnectorStepResolution::Complete(_)
        | connector::ConnectorStepResolution::Consume(_) => Ok(OrdinaryConnectorEffect::Accepted),
        connector::ConnectorStepResolution::Failed(failure)
            if failure.classification
                == WorkflowStepFailureClassification::ProviderIndeterminate =>
        {
            Ok(OrdinaryConnectorEffect::Failed(format!(
                "Workflow cancellation compensation target {:?} has an indeterminate ordinary invocation",
                step.plan.id
            )))
        }
        connector::ConnectorStepResolution::Await(_)
        | connector::ConnectorStepResolution::Wait { .. }
        | connector::ConnectorStepResolution::Failed(_) => {
            Ok(OrdinaryConnectorEffect::Unavailable)
        }
    }
}

fn normal_effective_input(
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    context: &WorkflowContext<'_>,
) -> Result<Option<serde_json::Value>, FlowError> {
    let prefix = format!("workflow-connector:{}:", step.plan.id);
    for envelope in context.history() {
        let FlowEvent::HookCreated {
            hook_id,
            token,
            metadata,
        } = &envelope.event
        else {
            continue;
        };
        if !hook_id.starts_with(&prefix) {
            continue;
        }
        let observed = serde_json::from_value::<WorkflowConnectorHookMetadata>(metadata.clone())?;
        if !matches!(observed.purpose, WorkflowConnectorInvocationPurpose::Normal) {
            continue;
        }
        let expected = WorkflowConnectorHookMetadata::from_run_step(
            input,
            step,
            observed.effective_input.clone(),
            observed.step_attempt,
            observed.observation,
        )
        .map_err(FlowError::InvalidWorkflow)?;
        if hook_id != &expected.flow_hook_id()
            || token != &expected.flow_hook_token()
            || observed != expected
        {
            return Err(FlowError::NonDeterministic {
                run_id: input.workflow_run_id.to_string(),
                reason: format!(
                    "Workflow Connector {:?} cancellation source authority drifted",
                    step.plan.id
                ),
            });
        }
        return Ok(Some(expected.effective_input));
    }
    Ok(None)
}
