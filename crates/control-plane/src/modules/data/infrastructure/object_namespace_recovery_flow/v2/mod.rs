mod delete;
mod restore;
mod seal;
pub(super) mod steps;

use super::{unknown_workflow, ObjectNamespaceRecoveryFlowRuntime, RecoveryStepOutput};
use crate::modules::data::application::{
    DeleteObjectNamespaceOperationInput, RestoreObjectNamespaceOperationInput,
    SealObjectNamespaceOperationInput, OBJECT_NAMESPACE_DELETE_WORKFLOW_NAME,
    OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION, OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME,
    OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME,
};
use a3s_flow::{FlowError, RuntimeCommand, StepInvocation, WorkflowContext, WorkflowInvocation};
use serde::Serialize;

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
            seal::replay(runtime, &context, input)
        }
        OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME => {
            let input = context.input_as::<RestoreObjectNamespaceOperationInput>()?;
            validate_run_id(&context, input.operation_id)?;
            input.validate().map_err(FlowError::Runtime)?;
            restore::replay(runtime, &context, input)
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
            delete::replay(runtime, &context, input)
        }
        _ => Err(unknown_workflow(&invocation)),
    }
}

pub(super) async fn run_step(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    invocation: StepInvocation,
) -> a3s_flow::Result<serde_json::Value> {
    match invocation.step_name.as_str() {
        super::OBJECT_NAMESPACE_SEAL_PAGE => {
            steps::seal_page(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_SEAL_VERIFY_PAGE => {
            steps::seal_verify_page(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_SEAL_FINALIZE => {
            steps::seal_finalize(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_RESTORE_PREFLIGHT_PAGE => {
            steps::restore_preflight_page(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_RESTORE_APPLY_PAGE => {
            steps::restore_apply_page(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_RESTORE_VERIFY_PAGE => {
            steps::restore_verify_page(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_RESTORE_FINALIZE => {
            steps::restore_finalize(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_DELETE_RETAINED_PREFLIGHT_PAGE => {
            steps::delete_retained_preflight_page(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_DELETE_SOURCE_PREFLIGHT_PAGE => {
            steps::delete_source_preflight_page(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_DELETE_MARK => {
            steps::delete_mark(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_DELETE_SOURCE_PAGE => {
            steps::delete_source_page(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_DELETE_SOURCE_ABSENCE => {
            steps::delete_source_absence(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_DELETE_RECOVERY_PLAN_PAGE => {
            steps::delete_recovery_plan_page(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_DELETE_RECOVERY_PAGE => {
            steps::delete_recovery_page(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_DELETE_RETAINED_POSTFLIGHT_PAGE => {
            steps::delete_retained_postflight_page(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_DELETE_RECOVERY_ANCHOR => {
            steps::delete_recovery_anchor(runtime, invocation.input_as()?).await
        }
        super::OBJECT_NAMESPACE_DELETE_FINALIZE => {
            steps::delete_finalize(runtime, invocation.input_as()?).await
        }
        step => Err(FlowError::Runtime(format!(
            "Cloud object namespace recovery workflow has no step {step:?}"
        ))),
    }
}

#[allow(clippy::large_enum_variant)]
pub(super) enum ReplayStep<T> {
    Output(T),
    Command(RuntimeCommand),
}

pub(super) fn replay_step<T: serde::de::DeserializeOwned + Serialize>(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    context: &WorkflowContext<'_>,
    step_id: &str,
    step_name: &str,
    input: impl Serialize,
) -> a3s_flow::Result<ReplayStep<T>> {
    match context.step_output_as::<RecoveryStepOutput<T>>(step_id)? {
        Some(RecoveryStepOutput::Completed { output }) => Ok(ReplayStep::Output(output)),
        Some(RecoveryStepOutput::Rejected { reason }) => {
            Ok(ReplayStep::Command(context.fail(reason)))
        }
        None => {
            if let Some(error) = context.step_failed(step_id) {
                return Ok(ReplayStep::Command(context.fail(format!(
                    "object namespace recovery step {step_name} exhausted retries: {error}"
                ))));
            }
            Ok(ReplayStep::Command(context.schedule_step_with_retry(
                step_id,
                step_name,
                serde_json::to_value(input)?,
                runtime.retry_policy(context),
            )))
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

pub(super) fn page_step_id(prefix: &str, page_index: u32) -> String {
    format!("{prefix}-{page_index:04}")
}
