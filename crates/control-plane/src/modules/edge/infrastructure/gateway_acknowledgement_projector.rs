use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::fleet::application::IGatewayAcknowledgementProjector;
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::NodeGatewayAck;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

pub struct EdgeGatewayAcknowledgementProjector {
    routes: Arc<dyn IEdgeRepository>,
}

impl EdgeGatewayAcknowledgementProjector {
    pub fn new(routes: Arc<dyn IEdgeRepository>) -> Self {
        Self { routes }
    }
}

#[async_trait]
impl IGatewayAcknowledgementProjector for EdgeGatewayAcknowledgementProjector {
    async fn project(
        &self,
        acknowledgement: &NodeGatewayAck,
        received_at: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        self.routes
            .project_gateway_acknowledgement(acknowledgement, received_at)
            .await
            .map(|_| ())
    }
}
