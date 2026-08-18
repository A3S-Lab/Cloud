use super::{
    ObjectNamespaceRecoveryFlowRuntime, RecoveryStepOutput, OBJECT_NAMESPACE_DELETE,
    OBJECT_NAMESPACE_RESTORE, OBJECT_NAMESPACE_SEAL,
};
use crate::modules::data::application::{
    DeleteObjectNamespaceOperationInput, DeleteObjectNamespaceOperationOutput,
    RestoreObjectNamespaceOperationInput, RestoreObjectNamespaceOperationOutput,
    SealObjectNamespaceOperationInput, SealObjectNamespaceOperationOutput,
    OBJECT_NAMESPACE_DELETE_WORKFLOW_NAME, OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION,
    OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME, OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME,
};
use a3s_flow::{FlowError, RuntimeCommand, WorkflowContext, WorkflowInvocation};
use serde::Serialize;

const STEP_ID: &str = "execute";
const DELETION_GRACE_WAIT_ID: &str = "deletion-grace";

pub(super) fn replay(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    invocation: WorkflowInvocation,
) -> a3s_flow::Result<RuntimeCommand> {
    if invocation.spec.version != OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION {
        return Err(unknown_workflow(&invocation));
    }
    let context = invocation.context();
    match invocation.spec.name.as_str() {
        OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME => {
            let input = context.input_as::<SealObjectNamespaceOperationInput>()?;
            validate_run_id(&context, input.operation_id)?;
            input.validate().map_err(FlowError::Runtime)?;
            replay_step::<SealObjectNamespaceOperationOutput>(
                runtime,
                &context,
                OBJECT_NAMESPACE_SEAL,
                input,
            )
        }
        OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME => {
            let input = context.input_as::<RestoreObjectNamespaceOperationInput>()?;
            validate_run_id(&context, input.operation_id)?;
            input.validate().map_err(FlowError::Runtime)?;
            replay_step::<RestoreObjectNamespaceOperationOutput>(
                runtime,
                &context,
                OBJECT_NAMESPACE_RESTORE,
                input,
            )
        }
        OBJECT_NAMESPACE_DELETE_WORKFLOW_NAME => {
            let input = context.input_as::<DeleteObjectNamespaceOperationInput>()?;
            validate_run_id(&context, input.operation_id)?;
            input.validate().map_err(FlowError::Runtime)?;
            if !context.wait_completed(DELETION_GRACE_WAIT_ID) {
                return Ok(context.wait_until(
                    DELETION_GRACE_WAIT_ID,
                    input.deletion_plan.spec().not_before,
                ));
            }
            replay_step::<DeleteObjectNamespaceOperationOutput>(
                runtime,
                &context,
                OBJECT_NAMESPACE_DELETE,
                input,
            )
        }
        _ => Err(unknown_workflow(&invocation)),
    }
}

fn replay_step<T: serde::de::DeserializeOwned + Serialize>(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    context: &WorkflowContext<'_>,
    step_name: &str,
    input: impl Serialize,
) -> a3s_flow::Result<RuntimeCommand> {
    match context.step_output_as::<RecoveryStepOutput<T>>(STEP_ID)? {
        Some(RecoveryStepOutput::Completed { output }) => {
            Ok(context.complete(serde_json::to_value(output)?))
        }
        Some(RecoveryStepOutput::Rejected { reason }) => Ok(context.fail(reason)),
        None => {
            if let Some(error) = context.step_failed(STEP_ID) {
                return Ok(context.fail(format!(
                    "object namespace recovery step {step_name} exhausted retries: {error}"
                )));
            }
            Ok(context.schedule_step_with_retry(
                STEP_ID,
                step_name,
                serde_json::to_value(input)?,
                runtime.retry_policy(),
            ))
        }
    }
}

fn validate_run_id(
    context: &WorkflowContext<'_>,
    operation_id: crate::modules::shared_kernel::domain::OperationId,
) -> a3s_flow::Result<()> {
    if context.run_id() != operation_id.to_string() {
        return Err(FlowError::Runtime(
            "object namespace recovery Flow run does not match its Operation ID".into(),
        ));
    }
    Ok(())
}

fn unknown_workflow(invocation: &WorkflowInvocation) -> FlowError {
    FlowError::Runtime(format!(
        "Cloud has no object namespace recovery workflow runtime for {}@{}",
        invocation.spec.name, invocation.spec.version
    ))
}
