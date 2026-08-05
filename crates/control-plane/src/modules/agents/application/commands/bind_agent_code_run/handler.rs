use super::BindAgentCodeRun;
use crate::modules::agents::domain::{AgentCodeRunWrite, BindAgentCodeRunWrite, IAgentRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct BindAgentCodeRunHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl BindAgentCodeRunHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }
}

impl CommandHandler<BindAgentCodeRun> for BindAgentCodeRunHandler {
    fn execute(
        &self,
        command: BindAgentCodeRun,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AgentCodeRunWrite>>> {
        let agents = Arc::clone(&self.agents);
        Box::pin(async move {
            let write = BindAgentCodeRunWrite {
                organization_id: command.organization_id,
                execution_id: command.execution_id,
                binding: command.binding,
            };
            if let Err(error) = write.validate() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            match agents.bind_code_run(write).await {
                Ok(result) => Ok(Ok(result)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
