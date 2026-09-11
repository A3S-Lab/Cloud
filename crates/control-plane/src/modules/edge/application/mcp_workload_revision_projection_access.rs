use crate::modules::edge::domain::EdgeMcpServiceProfileProjectionBinding;
use crate::modules::edge::domain::EdgeMcpWorkloadRevisionProjectionBinding;
use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError, WorkloadId};
use async_trait::async_trait;

/// Exact Workloads identity required to materialize an Edge MCP revision
/// projection binding for Gateway compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeMcpWorkloadRevisionProjectionScope {
    organization_id: OrganizationId,
    workload_id: WorkloadId,
}

impl EdgeMcpWorkloadRevisionProjectionScope {
    pub fn new(organization_id: OrganizationId, workload_id: WorkloadId) -> Result<Self, String> {
        let scope = Self {
            organization_id,
            workload_id,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil() || self.workload_id.as_uuid().is_nil() {
            return Err(
                "Edge MCP Workload revision projection scope requires non-nil identities".into(),
            );
        }
        Ok(())
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn workload_id(self) -> WorkloadId {
        self.workload_id
    }
}

/// Edge-owned read port for active MCP Workload revision projection bindings.
#[async_trait]
pub trait IEdgeMcpWorkloadRevisionProjectionAccess: Send + Sync {
    async fn find_active_revision_binding(
        &self,
        scope: EdgeMcpWorkloadRevisionProjectionScope,
        profile: &EdgeMcpServiceProfileProjectionBinding,
    ) -> Result<EdgeMcpWorkloadRevisionProjectionBinding, RepositoryError>;
}
