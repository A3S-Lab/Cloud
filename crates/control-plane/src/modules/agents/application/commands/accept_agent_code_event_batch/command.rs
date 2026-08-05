use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{NodeId, OrganizationId};
use a3s_boot::Command;
use a3s_cloud_contracts::{NodeCodeAgentEventBatchV1, NodeCodeAgentEventReceiptV1};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct AcceptAgentCodeEventBatch {
    pub authenticated_organization_id: OrganizationId,
    pub authenticated_node_id: NodeId,
    pub batch: NodeCodeAgentEventBatchV1,
    pub received_at: DateTime<Utc>,
}

impl Command for AcceptAgentCodeEventBatch {
    type Output = ApplicationResult<NodeCodeAgentEventReceiptV1>;
}
