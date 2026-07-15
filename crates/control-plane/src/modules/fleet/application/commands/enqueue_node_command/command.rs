use crate::modules::fleet::domain::entities::{NodeCommand, NodeCommandDraft};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::Command;

#[derive(Debug, Clone)]
pub struct EnqueueNodeCommand {
    pub draft: NodeCommandDraft,
}

impl Command for EnqueueNodeCommand {
    type Output = ApplicationResult<EnqueueNodeCommandResult>;
}

#[derive(Debug, Clone)]
pub struct EnqueueNodeCommandResult {
    pub command: NodeCommand,
    pub replayed: bool,
}
