use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{NodeId, OrganizationId};
use a3s_boot::Command;
use a3s_cloud_contracts::{InferenceUsageBatchV1, InferenceUsageReceiptV1};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct AcceptInferenceUsageBatch {
    pub authenticated_organization_id: OrganizationId,
    pub authenticated_node_id: NodeId,
    pub batch: InferenceUsageBatchV1,
    pub received_at: DateTime<Utc>,
}

impl Command for AcceptInferenceUsageBatch {
    type Output = ApplicationResult<InferenceUsageReceiptV1>;
}
