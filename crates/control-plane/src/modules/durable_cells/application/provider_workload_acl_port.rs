use super::workload_port::DurableCellWorkloadTemplate;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, NodePoolId, OrganizationId, ProjectId};
use async_trait::async_trait;

/// Exact tenant scope plus the public provider-workload ACL document admitted
/// at the Durable Cells inbound edge. Workloads remains the authority for ACL
/// schema, OCI resolution, and Service-template digesting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellProviderWorkloadAclRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub provider_workload_acl: String,
}

impl DurableCellProviderWorkloadAclRequest {
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        provider_workload_acl: impl Into<String>,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            environment_id,
            provider_workload_acl: provider_workload_acl.into(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
        {
            return Err("Durable Cell provider-workload ACL identity is invalid".into());
        }
        if self.provider_workload_acl.is_empty()
            || self.provider_workload_acl.len() > 64 * 1024
            || self.provider_workload_acl.contains('\0')
        {
            return Err("Durable Cell provider-workload ACL is out of bounds".into());
        }
        Ok(())
    }
}

/// Aggregate-free admission result: an opaque Workloads template fence plus
/// the optional placement pool selected by the public ACL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellProviderWorkloadAdmission {
    pub workload_template: DurableCellWorkloadTemplate,
    pub node_pool_id: Option<NodePoolId>,
}

impl DurableCellProviderWorkloadAdmission {
    pub fn new(
        workload_template: DurableCellWorkloadTemplate,
        node_pool_id: Option<NodePoolId>,
    ) -> Result<Self, String> {
        if node_pool_id.is_some_and(|node_pool_id| node_pool_id.as_uuid().is_nil()) {
            return Err("Durable Cell provider-workload ACL node pool is invalid".into());
        }
        Ok(Self {
            workload_template,
            node_pool_id,
        })
    }
}

/// Consumer-owned port that translates one public provider-workload ACL into
/// Durable Cells Application language. Exactly one Infrastructure adapter may
/// call Workloads ACL parsing and OCI resolution.
#[async_trait]
pub trait IDurableCellProviderWorkloadAclPort: Send + Sync {
    async fn admit(
        &self,
        request: &DurableCellProviderWorkloadAclRequest,
    ) -> ApplicationResult<DurableCellProviderWorkloadAdmission>;
}
