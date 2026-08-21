use super::{page_step_id, replay_step, steps, ReplayStep};
use crate::modules::data::application::{
    ObjectNamespaceSealPageCheckpoint, SealObjectNamespaceOperationInput,
    SealObjectNamespaceOperationOutput, OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES,
};
use crate::modules::data::infrastructure::object_namespace_recovery_flow::{
    ObjectNamespaceRecoveryFlowRuntime, OBJECT_NAMESPACE_SEAL_FINALIZE, OBJECT_NAMESPACE_SEAL_PAGE,
    OBJECT_NAMESPACE_SEAL_VERIFY_PAGE,
};
use a3s_flow::{FlowError, RuntimeCommand, WorkflowContext};

pub(super) fn replay(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    context: &WorkflowContext<'_>,
    operation: SealObjectNamespaceOperationInput,
) -> a3s_flow::Result<RuntimeCommand> {
    let mut pages = Vec::new();
    for page_index in 0..OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES {
        let step_id = page_step_id("seal-snapshot", page_index);
        let checkpoint = match replay_step::<ObjectNamespaceSealPageCheckpoint>(
            runtime,
            context,
            &step_id,
            OBJECT_NAMESPACE_SEAL_PAGE,
            steps::SealPageInput {
                operation: operation.clone(),
                page_index,
                previous: pages.last().cloned(),
            },
        )? {
            ReplayStep::Output(output) => output,
            ReplayStep::Command(command) => return Ok(command),
        };
        if checkpoint.page_index() != page_index {
            return Err(FlowError::Runtime(
                "object namespace seal checkpoint changed its page index".into(),
            ));
        }
        let complete = checkpoint.is_complete();
        pages.push(checkpoint);
        if complete {
            break;
        }
    }
    if !pages
        .last()
        .is_some_and(ObjectNamespaceSealPageCheckpoint::is_complete)
    {
        return Ok(context.fail("object namespace seal exceeded its durable page bound"));
    }

    for checkpoint in &pages {
        let step_id = page_step_id("seal-verify", checkpoint.page_index());
        match replay_step::<ObjectNamespaceSealPageCheckpoint>(
            runtime,
            context,
            &step_id,
            OBJECT_NAMESPACE_SEAL_VERIFY_PAGE,
            steps::SealVerifyPageInput {
                operation: operation.clone(),
                checkpoint: checkpoint.clone(),
            },
        )? {
            ReplayStep::Output(output) if output == *checkpoint => {}
            ReplayStep::Output(_) => {
                return Err(FlowError::Runtime(
                    "object namespace seal verification changed its checkpoint".into(),
                ));
            }
            ReplayStep::Command(command) => return Ok(command),
        }
    }

    match replay_step::<SealObjectNamespaceOperationOutput>(
        runtime,
        context,
        "seal-finalize",
        OBJECT_NAMESPACE_SEAL_FINALIZE,
        steps::SealFinalizeInput { operation, pages },
    )? {
        ReplayStep::Output(output) => Ok(context.complete(serde_json::to_value(output)?)),
        ReplayStep::Command(command) => Ok(command),
    }
}
