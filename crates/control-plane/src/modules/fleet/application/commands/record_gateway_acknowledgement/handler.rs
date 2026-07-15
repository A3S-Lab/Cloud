use super::RecordGatewayAcknowledgement;
use crate::modules::fleet::application::IGatewayAcknowledgementProjector;
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{NodeCommandId, NodeId};
use a3s_boot::{CommandHandler, CqrsContext};
use a3s_cloud_contracts::NodeCommandPayload;
use std::sync::Arc;

pub struct RecordGatewayAcknowledgementHandler {
    nodes: Arc<dyn INodeControlRepository>,
    projector: Arc<dyn IGatewayAcknowledgementProjector>,
}

impl RecordGatewayAcknowledgementHandler {
    pub fn new(
        nodes: Arc<dyn INodeControlRepository>,
        projector: Arc<dyn IGatewayAcknowledgementProjector>,
    ) -> Self {
        Self { nodes, projector }
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
        let projector = Arc::clone(&self.projector);
        Box::pin(async move {
            if command.acknowledgement.node_id != command.authenticated_node_id.as_uuid() {
                return Ok(Err(ApplicationError::Forbidden(
                    "authenticated certificate does not belong to the Gateway acknowledgement"
                        .into(),
                )));
            }
            if let Err(error) = command.acknowledgement.validate() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            let node_id = NodeId::from_uuid(command.acknowledgement.node_id);
            let command_id = NodeCommandId::from_uuid(command.acknowledgement.command_id);
            let issued = match nodes.find_command(node_id, command_id).await {
                Ok(Some(issued)) => issued,
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "Gateway publication command not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let NodeCommandPayload::GatewaySnapshotInstall { snapshot } = &issued.payload else {
                return Ok(Err(ApplicationError::Conflict(
                    "Gateway acknowledgement references a non-Gateway command".into(),
                )));
            };
            if let Err(error) = command.acknowledgement.validate_for(
                issued.id.as_uuid(),
                issued.node_id.as_uuid(),
                snapshot,
            ) {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            if command.acknowledgement.acknowledged_at < issued.issued_at {
                return Ok(Err(ApplicationError::Conflict(
                    "Gateway acknowledgement predates its publication command".into(),
                )));
            }
            let acknowledgement = command.acknowledgement;
            let receipt = match nodes
                .record_gateway_acknowledgement(acknowledgement.clone(), command.received_at)
                .await
            {
                Ok(receipt) => receipt,
                Err(error) => return Ok(Err(error.into())),
            };
            if let Err(error) = projector
                .project(&acknowledgement, command.received_at)
                .await
            {
                return Ok(Err(error.into()));
            }
            Ok(Ok(receipt))
        })
    }
}
