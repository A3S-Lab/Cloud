use super::RecordNodeResourceInventory;
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct RecordNodeResourceInventoryHandler {
    nodes: Arc<dyn INodeControlRepository>,
}

impl RecordNodeResourceInventoryHandler {
    pub fn new(nodes: Arc<dyn INodeControlRepository>) -> Self {
        Self { nodes }
    }
}

impl CommandHandler<RecordNodeResourceInventory> for RecordNodeResourceInventoryHandler {
    fn execute(
        &self,
        command: RecordNodeResourceInventory,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<a3s_cloud_contracts::NodeResourceInventoryReceipt>>,
    > {
        let nodes = Arc::clone(&self.nodes);
        Box::pin(async move {
            if command.inventory.node_id != command.authenticated_node_id.as_uuid() {
                return Ok(Err(ApplicationError::Forbidden(
                    "authenticated certificate does not belong to the resource inventory".into(),
                )));
            }
            Ok(
                match nodes
                    .record_resource_inventory(command.inventory, command.received_at)
                    .await
                {
                    Ok(receipt) => Ok(receipt),
                    Err(error) => Err(error.into()),
                },
            )
        })
    }
}
