use super::{page_step_id, replay_step, steps, ReplayStep};
use crate::modules::data::application::{
    DeleteObjectNamespaceOperationInput, DeleteObjectNamespaceOperationOutput,
    ObjectNamespaceCleanupPageCheckpoint, ObjectNamespaceManifestPageCheckpoint,
    ObjectNamespaceObservationPageCheckpoint, ObjectNamespaceRecoveryAnchorCheckpoint,
    OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES,
};
use crate::modules::data::infrastructure::object_namespace_recovery_flow::{
    ObjectNamespaceRecoveryFlowRuntime, OBJECT_NAMESPACE_DELETE_FINALIZE,
    OBJECT_NAMESPACE_DELETE_MARK, OBJECT_NAMESPACE_DELETE_RECOVERY_ANCHOR,
    OBJECT_NAMESPACE_DELETE_RECOVERY_PAGE, OBJECT_NAMESPACE_DELETE_RECOVERY_PLAN_PAGE,
    OBJECT_NAMESPACE_DELETE_RETAINED_POSTFLIGHT_PAGE,
    OBJECT_NAMESPACE_DELETE_RETAINED_PREFLIGHT_PAGE, OBJECT_NAMESPACE_DELETE_SOURCE_ABSENCE,
    OBJECT_NAMESPACE_DELETE_SOURCE_PAGE, OBJECT_NAMESPACE_DELETE_SOURCE_PREFLIGHT_PAGE,
};
use a3s_flow::{FlowError, RuntimeCommand, WorkflowContext};

pub(super) fn replay(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    context: &WorkflowContext<'_>,
    operation: DeleteObjectNamespaceOperationInput,
) -> a3s_flow::Result<RuntimeCommand> {
    let retained_preflight = match replay_retained_pages(
        runtime,
        context,
        &operation,
        "delete-retained-preflight",
        OBJECT_NAMESPACE_DELETE_RETAINED_PREFLIGHT_PAGE,
    )? {
        ReplayStep::Output(output) => output,
        ReplayStep::Command(command) => return Ok(command),
    };
    let source_preflight = match replay_source_preflight(runtime, context, &operation)? {
        ReplayStep::Output(output) => output,
        ReplayStep::Command(command) => return Ok(command),
    };
    match replay_step::<()>(
        runtime,
        context,
        "delete-mark",
        OBJECT_NAMESPACE_DELETE_MARK,
        steps::DeleteMarkInput {
            operation: operation.clone(),
            retained_checkpoint: retained_preflight,
            source_checkpoint: source_preflight,
        },
    )? {
        ReplayStep::Output(()) => {}
        ReplayStep::Command(command) => return Ok(command),
    }
    match replay_source_pages(runtime, context, &operation)? {
        ReplayStep::Output(_) => {}
        ReplayStep::Command(command) => return Ok(command),
    }
    match replay_step::<ObjectNamespaceObservationPageCheckpoint>(
        runtime,
        context,
        "delete-source-absence",
        OBJECT_NAMESPACE_DELETE_SOURCE_ABSENCE,
        operation.clone(),
    )? {
        ReplayStep::Output(checkpoint) if checkpoint.is_complete() => {}
        ReplayStep::Output(_) => {
            return Err(FlowError::Runtime(
                "object namespace source absence checkpoint is incomplete".into(),
            ));
        }
        ReplayStep::Command(command) => return Ok(command),
    }
    match replay_recovery_cleanup(runtime, context, &operation)? {
        ReplayStep::Output(_) => {}
        ReplayStep::Command(command) => return Ok(command),
    }
    let retained_postflight = match replay_retained_pages(
        runtime,
        context,
        &operation,
        "delete-retained-postflight",
        OBJECT_NAMESPACE_DELETE_RETAINED_POSTFLIGHT_PAGE,
    )? {
        ReplayStep::Output(output) => output,
        ReplayStep::Command(command) => return Ok(command),
    };
    let anchor = match replay_step::<ObjectNamespaceRecoveryAnchorCheckpoint>(
        runtime,
        context,
        "delete-recovery-anchor",
        OBJECT_NAMESPACE_DELETE_RECOVERY_ANCHOR,
        operation.clone(),
    )? {
        ReplayStep::Output(output) => output,
        ReplayStep::Command(command) => return Ok(command),
    };
    match replay_step::<DeleteObjectNamespaceOperationOutput>(
        runtime,
        context,
        "delete-finalize",
        OBJECT_NAMESPACE_DELETE_FINALIZE,
        steps::DeleteFinalizeInput {
            operation,
            retained_checkpoint: retained_postflight,
            anchor_checkpoint: anchor,
        },
    )? {
        ReplayStep::Output(output) => Ok(context.complete(serde_json::to_value(output)?)),
        ReplayStep::Command(command) => Ok(command),
    }
}

fn replay_retained_pages(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    context: &WorkflowContext<'_>,
    operation: &DeleteObjectNamespaceOperationInput,
    step_prefix: &str,
    step_name: &str,
) -> a3s_flow::Result<ReplayStep<ObjectNamespaceManifestPageCheckpoint>> {
    let mut previous = None;
    for page_index in 0..OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES {
        let step_id = page_step_id(step_prefix, page_index);
        let checkpoint = match replay_step::<ObjectNamespaceManifestPageCheckpoint>(
            runtime,
            context,
            &step_id,
            step_name,
            steps::DeleteRetainedPageInput {
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
                "object namespace retained checkpoint changed its page index".into(),
            ));
        }
        if checkpoint.is_complete() {
            return Ok(ReplayStep::Output(checkpoint));
        }
        previous = Some(checkpoint);
    }
    Err(FlowError::Runtime(
        "object namespace retained verification exceeded its durable page bound".into(),
    ))
}

fn replay_source_preflight(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    context: &WorkflowContext<'_>,
    operation: &DeleteObjectNamespaceOperationInput,
) -> a3s_flow::Result<ReplayStep<ObjectNamespaceObservationPageCheckpoint>> {
    let mut previous = None;
    for page_index in 0..OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES {
        let step_id = page_step_id("delete-source-preflight", page_index);
        let checkpoint = match replay_step::<ObjectNamespaceObservationPageCheckpoint>(
            runtime,
            context,
            &step_id,
            OBJECT_NAMESPACE_DELETE_SOURCE_PREFLIGHT_PAGE,
            steps::DeleteSourcePreflightPageInput {
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
                "object namespace source preflight changed its page index".into(),
            ));
        }
        if checkpoint.is_complete() {
            return Ok(ReplayStep::Output(checkpoint));
        }
        previous = Some(checkpoint);
    }
    Err(FlowError::Runtime(
        "object namespace source preflight exceeded its durable page bound".into(),
    ))
}

fn replay_source_pages(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    context: &WorkflowContext<'_>,
    operation: &DeleteObjectNamespaceOperationInput,
) -> a3s_flow::Result<ReplayStep<ObjectNamespaceManifestPageCheckpoint>> {
    let mut previous = None;
    for page_index in 0..OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES {
        let step_id = page_step_id("delete-source", page_index);
        let checkpoint = match replay_step::<ObjectNamespaceManifestPageCheckpoint>(
            runtime,
            context,
            &step_id,
            OBJECT_NAMESPACE_DELETE_SOURCE_PAGE,
            steps::DeleteSourcePageInput {
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
                "object namespace source deletion changed its page index".into(),
            ));
        }
        if checkpoint.is_complete() {
            return Ok(ReplayStep::Output(checkpoint));
        }
        previous = Some(checkpoint);
    }
    Err(FlowError::Runtime(
        "object namespace source deletion exceeded its durable page bound".into(),
    ))
}

fn replay_recovery_cleanup(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    context: &WorkflowContext<'_>,
    operation: &DeleteObjectNamespaceOperationInput,
) -> a3s_flow::Result<ReplayStep<ObjectNamespaceCleanupPageCheckpoint>> {
    let mut previous = None;
    for page_index in 0..OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES {
        let plan_step_id = page_step_id("delete-recovery-plan", page_index);
        let planned = match replay_step::<ObjectNamespaceCleanupPageCheckpoint>(
            runtime,
            context,
            &plan_step_id,
            OBJECT_NAMESPACE_DELETE_RECOVERY_PLAN_PAGE,
            steps::DeleteRecoveryPlanPageInput {
                operation: operation.clone(),
                page_index,
                previous: previous.clone(),
            },
        )? {
            ReplayStep::Output(output) => output,
            ReplayStep::Command(command) => return Ok(ReplayStep::Command(command)),
        };
        if planned.page_index() != page_index {
            return Err(FlowError::Runtime(
                "object namespace recovery cleanup plan changed its page index".into(),
            ));
        }
        let delete_step_id = page_step_id("delete-recovery", page_index);
        let deleted = match replay_step::<ObjectNamespaceCleanupPageCheckpoint>(
            runtime,
            context,
            &delete_step_id,
            OBJECT_NAMESPACE_DELETE_RECOVERY_PAGE,
            steps::DeleteRecoveryPageInput {
                operation: operation.clone(),
                checkpoint: planned.clone(),
            },
        )? {
            ReplayStep::Output(output) => output,
            ReplayStep::Command(command) => return Ok(ReplayStep::Command(command)),
        };
        if deleted != planned {
            return Err(FlowError::Runtime(
                "object namespace recovery cleanup changed its durable plan".into(),
            ));
        }
        if deleted.is_complete() {
            return Ok(ReplayStep::Output(deleted));
        }
        previous = Some(deleted);
    }
    Err(FlowError::Runtime(
        "object namespace recovery cleanup exceeded its durable page bound".into(),
    ))
}
