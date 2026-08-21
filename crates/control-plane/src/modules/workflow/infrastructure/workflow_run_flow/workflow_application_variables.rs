use super::WorkflowLocalStepResult;
use crate::modules::workflow::domain::{
    ResolvedWorkflowRunStep, WorkflowApplicationVariableSnapshotHookMetadata,
    WorkflowApplicationVariableSnapshotResumePayload, WorkflowApplicationVariableWriteHookMetadata,
    WorkflowApplicationVariableWriteResumePayload, WorkflowStepKind,
};
use a3s_flow::FlowError;
use serde_json::Value;
pub(super) fn application_variable_snapshot_result(
    run_id: &str,
    hook_id: &str,
    step: &ResolvedWorkflowRunStep,
    metadata: &WorkflowApplicationVariableSnapshotHookMetadata,
    observed: &Value,
) -> Result<WorkflowApplicationVariableSnapshotResumePayload, FlowError> {
    let payload = serde_json::from_value::<WorkflowApplicationVariableSnapshotResumePayload>(
        observed.clone(),
    )
    .map_err(|_| application_variable_payload_drift(run_id, &step.plan.id, "snapshot"))?;
    payload
        .validate(metadata)
        .map_err(|_| application_variable_payload_drift(run_id, &step.plan.id, "snapshot"))?;
    if payload.flow_run_id != run_id || payload.flow_hook_id != hook_id {
        return Err(application_variable_payload_drift(
            run_id,
            &step.plan.id,
            "snapshot",
        ));
    }
    Ok(payload)
}

pub(super) fn application_variable_write_result(
    run_id: &str,
    hook_id: &str,
    step: &ResolvedWorkflowRunStep,
    metadata: &WorkflowApplicationVariableWriteHookMetadata,
    values: &Value,
    observed: &Value,
) -> Result<WorkflowLocalStepResult, FlowError> {
    let payload =
        serde_json::from_value::<WorkflowApplicationVariableWriteResumePayload>(observed.clone())
            .map_err(|_| application_variable_payload_drift(run_id, &step.plan.id, "write"))?;
    payload
        .validate(metadata)
        .map_err(|_| application_variable_payload_drift(run_id, &step.plan.id, "write"))?;
    if payload.flow_run_id != run_id
        || payload.flow_hook_id != hook_id
        || super::execution::value_digest(values, "Workflow Application variable assignment output")
            .map_err(|_| application_variable_payload_drift(run_id, &step.plan.id, "write"))?
            != metadata.values_digest
    {
        return Err(application_variable_payload_drift(
            run_id,
            &step.plan.id,
            "write",
        ));
    }
    let result = WorkflowLocalStepResult {
        step_id: step.plan.id.clone(),
        kind: WorkflowStepKind::Service,
        output: values.clone(),
        output_digest: metadata.values_digest.clone(),
        selected_handle: None,
        composite_region_result: None,
        default_output_evidence: None,
    };
    result
        .validate(step)
        .map_err(|_| application_variable_payload_drift(run_id, &step.plan.id, "write"))?;
    super::execution::validate_data_schema(
        &step.output_schema,
        &result.output,
        "Workflow Application variable assignment output",
    )
    .map_err(|_| application_variable_payload_drift(run_id, &step.plan.id, "write"))?;
    Ok(result)
}

fn application_variable_payload_drift(run_id: &str, step_id: &str, phase: &str) -> FlowError {
    FlowError::NonDeterministic {
        run_id: run_id.into(),
        reason: format!(
            "Workflow Application variable {phase} for step {step_id:?} received invalid authority-bound evidence"
        ),
    }
}
