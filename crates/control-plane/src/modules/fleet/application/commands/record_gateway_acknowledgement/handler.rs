use super::RecordGatewayAcknowledgement;
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct RecordGatewayAcknowledgementHandler {
    nodes: Arc<dyn INodeControlRepository>,
}

impl RecordGatewayAcknowledgementHandler {
    pub fn new(nodes: Arc<dyn INodeControlRepository>) -> Self {
        Self { nodes }
    }
}

impl CommandHandler<RecordGatewayAcknowledgement> for RecordGatewayAcknowledgementHandler {
    fn execute(
        &self,
        command: RecordGatewayAcknowledgement,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<a3s_cloud_contracts::NodeGatewayAckReceipt>>,
    > {
        let nodes = Arc::clone(&self.nodes);
        Box::pin(async move {
            if command.acknowledgement.node_id != command.authenticated_node_id.as_uuid() {
                return Ok(Err(ApplicationError::Forbidden(
                    "authenticated certificate does not belong to the Gateway acknowledgement"
                        .into(),
                )));
            }
            Ok(
                match nodes
                    .record_gateway_acknowledgement(command.acknowledgement, command.received_at)
                    .await
                {
                    Ok(receipt) => Ok(receipt),
                    Err(error) => Err(error.into()),
                },
            )
        })
    }
}
