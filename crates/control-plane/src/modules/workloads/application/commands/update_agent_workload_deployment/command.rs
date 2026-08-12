use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, NodePoolId, OrganizationId, WorkloadId,
};
use crate::modules::workloads::application::{
    SourceWorkloadTemplate, UpdateWorkloadDeploymentResult,
};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct UpdateAgentWorkloadDeployment {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub resource_access: ResourceAccessEvaluator,
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
    pub expected_name: Option<String>,
    pub expected_node_pool_id: Option<Option<NodePoolId>>,
    pub template: SourceWorkloadTemplate,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for UpdateAgentWorkloadDeployment {
    type Output = ApplicationResult<UpdateWorkloadDeploymentResult>;
}
