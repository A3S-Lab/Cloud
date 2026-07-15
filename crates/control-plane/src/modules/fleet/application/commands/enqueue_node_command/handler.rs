use super::{EnqueueNodeCommand, EnqueueNodeCommandResult};
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct EnqueueNodeCommandHandler {
    commands: Arc<dyn INodeControlRepository>,
}

impl EnqueueNodeCommandHandler {
    pub fn new(commands: Arc<dyn INodeControlRepository>) -> Self {
        Self { commands }
    }
}

impl CommandHandler<EnqueueNodeCommand> for EnqueueNodeCommandHandler {
    fn execute(
        &self,
        command: EnqueueNodeCommand,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<EnqueueNodeCommandResult>>>
    {
        let commands = Arc::clone(&self.commands);
        Box::pin(async move {
            Ok(match commands.enqueue_command(command.draft).await {
                Ok(result) => Ok(EnqueueNodeCommandResult {
                    command: result.value,
                    replayed: result.replayed,
                }),
                Err(error) => Err(error.into()),
            })
        })
    }
}
