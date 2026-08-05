use super::AcceptAgentCodeEventBatch;
use crate::modules::agents::domain::{AcceptAgentCodeEventBatchWrite, IAgentRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CommandHandler, CqrsContext};
use a3s_cloud_contracts::NodeCodeAgentEventReceiptV1;
use std::sync::Arc;

pub struct AcceptAgentCodeEventBatchHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl AcceptAgentCodeEventBatchHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }
}

impl CommandHandler<AcceptAgentCodeEventBatch> for AcceptAgentCodeEventBatchHandler {
    fn execute(
        &self,
        command: AcceptAgentCodeEventBatch,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<NodeCodeAgentEventReceiptV1>>,
    > {
        let agents = Arc::clone(&self.agents);
        Box::pin(async move {
            let write = match AcceptAgentCodeEventBatchWrite::new(
                command.authenticated_organization_id,
                command.authenticated_node_id,
                command.batch,
                command.received_at,
            ) {
                Ok(write) => write,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match agents.accept_code_event_batch(write).await {
                Ok(receipt) => Ok(Ok(receipt)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
