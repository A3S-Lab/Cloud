use super::RecordNodeObservations;
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct RecordNodeObservationsHandler {
    nodes: Arc<dyn INodeControlRepository>,
}

impl RecordNodeObservationsHandler {
    pub fn new(nodes: Arc<dyn INodeControlRepository>) -> Self {
        Self { nodes }
    }
}

impl CommandHandler<RecordNodeObservations> for RecordNodeObservationsHandler {
    fn execute(
        &self,
        command: RecordNodeObservations,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<a3s_cloud_contracts::NodeObservationReceipt>>,
    > {
        let nodes = Arc::clone(&self.nodes);
        Box::pin(async move {
            if command.batch.node_id != command.authenticated_node_id.as_uuid() {
                return Ok(Err(ApplicationError::Forbidden(
                    "authenticated certificate does not belong to the observation batch".into(),
                )));
            }
            Ok(
                match nodes
                    .record_observations(command.batch, command.received_at)
                    .await
                {
                    Ok(receipt) => Ok(receipt),
                    Err(error) => Err(error.into()),
                },
            )
        })
    }
}
