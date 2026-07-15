use super::{AcknowledgeNodeCommand, AcknowledgeNodeCommandResult};
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct AcknowledgeNodeCommandHandler {
    commands: Arc<dyn INodeControlRepository>,
}

impl AcknowledgeNodeCommandHandler {
    pub fn new(commands: Arc<dyn INodeControlRepository>) -> Self {
        Self { commands }
    }
}

impl CommandHandler<AcknowledgeNodeCommand> for AcknowledgeNodeCommandHandler {
    fn execute(
        &self,
        command: AcknowledgeNodeCommand,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcknowledgeNodeCommandResult>>,
    > {
        let commands = Arc::clone(&self.commands);
        Box::pin(async move {
            if command.acknowledgement.node_id != command.authenticated_node_id.as_uuid() {
                return Ok(Err(ApplicationError::Forbidden(
                    "authenticated certificate does not belong to the acknowledged command".into(),
                )));
            }
            Ok(
                match commands
                    .acknowledge_command(command.acknowledgement, command.received_at)
                    .await
                {
                    Ok(result) => Ok(AcknowledgeNodeCommandResult {
                        acknowledgement: result.value,
                        replayed: result.replayed,
                    }),
                    Err(error) => Err(error.into()),
                },
            )
        })
    }
}
