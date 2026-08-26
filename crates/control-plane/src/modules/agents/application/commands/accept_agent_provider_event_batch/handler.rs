use super::AcceptAgentProviderEventBatch;
use crate::modules::agents::domain::{AcceptAgentProviderEventBatchWrite, IAgentRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CommandHandler, CqrsContext};
use a3s_cloud_contracts::NodeAgentProviderEventReceiptV1;
use std::sync::Arc;

pub struct AcceptAgentProviderEventBatchHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl AcceptAgentProviderEventBatchHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }
}

impl CommandHandler<AcceptAgentProviderEventBatch> for AcceptAgentProviderEventBatchHandler {
    fn execute(
        &self,
        command: AcceptAgentProviderEventBatch,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<NodeAgentProviderEventReceiptV1>>,
    > {
        let agents = Arc::clone(&self.agents);
        Box::pin(async move {
            let write = match AcceptAgentProviderEventBatchWrite::new(
                command.authenticated_organization_id,
                command.authenticated_node_id,
                command.batch,
                command.received_at,
            ) {
                Ok(write) => write,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match agents.accept_provider_event_batch(write).await {
                Ok(receipt) => Ok(Ok(receipt)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
