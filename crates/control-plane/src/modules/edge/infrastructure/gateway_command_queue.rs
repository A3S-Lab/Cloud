use crate::modules::edge::domain::services::{GatewayCommandDispatch, IGatewayCommandQueue};
use crate::modules::edge::domain::GatewayPublication;
use crate::modules::fleet::{
    FleetGatewaySnapshotInstallRequest, IFleetGatewaySnapshotCommandPort,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::sync::Arc;

pub struct FleetGatewayCommandQueue {
    commands: Arc<dyn IFleetGatewaySnapshotCommandPort>,
}

impl FleetGatewayCommandQueue {
    pub fn new(commands: Arc<dyn IFleetGatewaySnapshotCommandPort>) -> Self {
        Self { commands }
    }
}

#[async_trait]
impl IGatewayCommandQueue for FleetGatewayCommandQueue {
    async fn enqueue(
        &self,
        publication: &GatewayPublication,
    ) -> Result<GatewayCommandDispatch, RepositoryError> {
        let dispatch = self
            .commands
            .enqueue_install(FleetGatewaySnapshotInstallRequest {
                node_id: publication.node_id,
                command_id: publication.command_id,
                correlation_id: publication.command_correlation_id,
                issued_at: publication.command_issued_at,
                not_after: publication.command_not_after,
                snapshot: publication.snapshot().map_err(RepositoryError::Conflict)?,
            })
            .await?;
        Ok(GatewayCommandDispatch {
            replayed: dispatch.replayed,
        })
    }
}
