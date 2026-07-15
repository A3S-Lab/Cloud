use crate::modules::edge::domain::services::{GatewayCommandDispatch, IGatewayCommandQueue};
use crate::modules::edge::domain::GatewayPublication;
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::NodeCommandPayload;
use async_trait::async_trait;
use std::sync::Arc;

pub struct FleetGatewayCommandQueue {
    commands: Arc<dyn INodeControlRepository>,
}

impl FleetGatewayCommandQueue {
    pub fn new(commands: Arc<dyn INodeControlRepository>) -> Self {
        Self { commands }
    }
}

#[async_trait]
impl IGatewayCommandQueue for FleetGatewayCommandQueue {
    async fn enqueue(
        &self,
        publication: &GatewayPublication,
    ) -> Result<GatewayCommandDispatch, RepositoryError> {
        let result = self
            .commands
            .enqueue_command(NodeCommandDraft {
                proposed_command_id: publication.command_id,
                node_id: publication.node_id,
                aggregate_id: publication.node_id.as_uuid(),
                payload: NodeCommandPayload::GatewaySnapshotInstall {
                    snapshot: Box::new(publication.snapshot().map_err(RepositoryError::Conflict)?),
                },
                issued_at: publication.command_issued_at,
                not_after: publication.command_not_after,
                correlation_id: publication.command_correlation_id,
            })
            .await?;
        Ok(GatewayCommandDispatch {
            replayed: result.replayed,
        })
    }
}
