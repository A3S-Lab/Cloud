use super::{NodeControlClient, NodeControlClientError};
use a3s_cloud_contracts::{
    NodeAgentProviderEventBatchV1, NodeAgentProviderEventReceiptV1, NodeCodeAgentEventBatchV1,
    NodeCodeAgentEventReceiptV1,
};

impl NodeControlClient {
    pub async fn record_code_agent_events(
        &self,
        batch: &NodeCodeAgentEventBatchV1,
    ) -> Result<NodeCodeAgentEventReceiptV1, NodeControlClientError> {
        batch.validate().map_err(NodeControlClientError::Invalid)?;
        if batch.node_id != self.node_id {
            return Err(NodeControlClientError::Invalid(
                "Code Agent event batch changed the authenticated node identity".into(),
            ));
        }
        let receipt: NodeCodeAgentEventReceiptV1 = self
            .send(
                self.client
                    .post(self.endpoint("v1/node-control/code-agent-events")?)
                    .timeout(self.request_timeout)
                    .json(batch),
            )
            .await?;
        receipt
            .validate_for(batch)
            .map_err(NodeControlClientError::Invalid)?;
        Ok(receipt)
    }

    pub async fn record_agent_provider_events(
        &self,
        batch: &NodeAgentProviderEventBatchV1,
    ) -> Result<NodeAgentProviderEventReceiptV1, NodeControlClientError> {
        batch.validate().map_err(NodeControlClientError::Invalid)?;
        if batch.node_id != self.node_id {
            return Err(NodeControlClientError::Invalid(
                "Agent provider event batch changed the authenticated node identity".into(),
            ));
        }
        let receipt: NodeAgentProviderEventReceiptV1 = self
            .send(
                self.client
                    .post(self.endpoint("v1/node-control/agent-provider-events")?)
                    .timeout(self.request_timeout)
                    .json(batch),
            )
            .await?;
        receipt
            .validate_for(batch)
            .map_err(NodeControlClientError::Invalid)?;
        Ok(receipt)
    }
}
