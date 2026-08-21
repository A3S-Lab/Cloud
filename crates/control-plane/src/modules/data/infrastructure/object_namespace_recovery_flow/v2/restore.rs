use super::{page_step_id, replay_step, steps, ReplayStep};
use crate::modules::data::application::{
    ObjectNamespaceManifestPageCheckpoint, ObjectNamespaceObservationPageCheckpoint,
    RestoreObjectNamespaceOperationInput, RestoreObjectNamespaceOperationOutput,
    OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES,
};
use crate::modules::data::infrastructure::object_namespace_recovery_flow::{
    ObjectNamespaceRecoveryFlowRuntime, OBJECT_NAMESPACE_RESTORE_APPLY_PAGE,
    OBJECT_NAMESPACE_RESTORE_FINALIZE, OBJECT_NAMESPACE_RESTORE_PREFLIGHT_PAGE,
    OBJECT_NAMESPACE_RESTORE_VERIFY_PAGE,
};
use a3s_flow::{FlowError, RuntimeCommand, WorkflowContext};

pub(super) fn replay(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    context: &WorkflowContext<'_>,
    operation: RestoreObjectNamespaceOperationInput,
) -> a3s_flow::Result<RuntimeCommand> {
    let preflight = match replay_observation_pages(runtime, context, &operation)? {
        ReplayStep::Output(output) => output,
        ReplayStep::Command(command) => return Ok(command),
    };
    match replay_manifest_pages(
        runtime,
        context,
        &operation,
        "restore-apply",
        OBJECT_NAMESPACE_RESTORE_APPLY_PAGE,
        steps::RestoreManifestPhase::Apply,
    )? {
        ReplayStep::Output(_) => {}
        ReplayStep::Command(command) => return Ok(command),
    }
    let verified = match replay_manifest_pages(
        runtime,
        context,
        &operation,
        "restore-verify",
        OBJECT_NAMESPACE_RESTORE_VERIFY_PAGE,
        steps::RestoreManifestPhase::Verify,
    )? {
        ReplayStep::Output(output) => output,
        ReplayStep::Command(command) => return Ok(command),
    };
    match replay_step::<RestoreObjectNamespaceOperationOutput>(
        runtime,
        context,
        "restore-finalize",
        OBJECT_NAMESPACE_RESTORE_FINALIZE,
        steps::RestoreFinalizeInput {
            operation,
            preflight,
            verification: verified,
        },
    )? {
        ReplayStep::Output(output) => Ok(context.complete(serde_json::to_value(output)?)),
        ReplayStep::Command(command) => Ok(command),
    }
}

fn replay_observation_pages(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    context: &WorkflowContext<'_>,
    operation: &RestoreObjectNamespaceOperationInput,
) -> a3s_flow::Result<ReplayStep<ObjectNamespaceObservationPageCheckpoint>> {
    let mut previous = None;
    for page_index in 0..OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES {
        let step_id = page_step_id("restore-preflight", page_index);
        let checkpoint = match replay_step::<ObjectNamespaceObservationPageCheckpoint>(
            runtime,
            context,
            &step_id,
            OBJECT_NAMESPACE_RESTORE_PREFLIGHT_PAGE,
            steps::RestorePreflightPageInput {
                operation: operation.clone(),
                page_index,
                previous: previous.clone(),
            },
        )? {
            ReplayStep::Output(output) => output,
            ReplayStep::Command(command) => return Ok(ReplayStep::Command(command)),
        };
        if checkpoint.page_index() != page_index {
            return Err(FlowError::Runtime(
                "object namespace restore preflight changed its page index".into(),
            ));
        }
        if checkpoint.is_complete() {
            return Ok(ReplayStep::Output(checkpoint));
        }
        previous = Some(checkpoint);
    }
    Err(FlowError::Runtime(
        "object namespace restore preflight exceeded its durable page bound".into(),
    ))
}

fn replay_manifest_pages(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    context: &WorkflowContext<'_>,
    operation: &RestoreObjectNamespaceOperationInput,
    step_prefix: &str,
    step_name: &str,
    phase: steps::RestoreManifestPhase,
) -> a3s_flow::Result<ReplayStep<ObjectNamespaceManifestPageCheckpoint>> {
    let mut previous = None;
    for page_index in 0..OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES {
        let step_id = page_step_id(step_prefix, page_index);
        let checkpoint = match replay_step::<ObjectNamespaceManifestPageCheckpoint>(
            runtime,
            context,
            &step_id,
            step_name,
            steps::RestoreManifestPageInput {
                operation: operation.clone(),
                phase,
                page_index,
                previous: previous.clone(),
            },
        )? {
            ReplayStep::Output(output) => output,
            ReplayStep::Command(command) => return Ok(ReplayStep::Command(command)),
        };
        if checkpoint.page_index() != page_index {
            return Err(FlowError::Runtime(
                "object namespace restore checkpoint changed its page index".into(),
            ));
        }
        if checkpoint.is_complete() {
            return Ok(ReplayStep::Output(checkpoint));
        }
        previous = Some(checkpoint);
    }
    Err(FlowError::Runtime(
        "object namespace restore phase exceeded its durable page bound".into(),
    ))
}
