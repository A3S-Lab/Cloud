use super::DeploymentFlowConfig;
use a3s_flow::{RuntimeCommand, WorkflowInvocation};

pub(super) fn replay(
    config: &DeploymentFlowConfig,
    invocation: WorkflowInvocation,
) -> a3s_flow::Result<RuntimeCommand> {
    super::workflow::replay_previous(config, invocation)
}
