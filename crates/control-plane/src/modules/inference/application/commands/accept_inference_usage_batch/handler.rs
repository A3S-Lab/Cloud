use super::AcceptInferenceUsageBatch;
use crate::modules::inference::domain::{AcceptInferenceUsageBatchWrite, IInferenceUsageRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CommandHandler, CqrsContext};
use a3s_cloud_contracts::InferenceUsageReceiptV1;
use std::sync::Arc;

pub struct AcceptInferenceUsageBatchHandler {
    usage: Arc<dyn IInferenceUsageRepository>,
}

impl AcceptInferenceUsageBatchHandler {
    pub fn new(usage: Arc<dyn IInferenceUsageRepository>) -> Self {
        Self { usage }
    }
}

impl CommandHandler<AcceptInferenceUsageBatch> for AcceptInferenceUsageBatchHandler {
    fn execute(
        &self,
        command: AcceptInferenceUsageBatch,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<InferenceUsageReceiptV1>>>
    {
        let usage = Arc::clone(&self.usage);
        Box::pin(async move {
            let write = match AcceptInferenceUsageBatchWrite::new(
                command.authenticated_organization_id,
                command.authenticated_node_id,
                command.batch,
                command.received_at,
            ) {
                Ok(write) => write,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match usage.accept_usage_batch(write).await {
                Ok(receipt) => Ok(Ok(receipt)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
