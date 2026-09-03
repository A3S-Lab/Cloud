use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, NodePoolId, OrganizationId, ProjectId};
use async_trait::async_trait;

/// Exact tenant scope and optional Fleet node-pool selection supplied by a
/// Durable Cell deployment. Fleet remains the authority for node-pool
/// existence and ownership; no Fleet aggregate crosses this boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurableCellNodePoolSelectionRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub node_pool_id: Option<NodePoolId>,
}

impl DurableCellNodePoolSelectionRequest {
    pub const fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        node_pool_id: Option<NodePoolId>,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            environment_id,
            node_pool_id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self
                .node_pool_id
                .is_some_and(|node_pool_id| node_pool_id.as_uuid().is_nil())
        {
            return Err("Durable Cell node-pool selection identity is invalid".into());
        }
        Ok(())
    }
}

/// Durable Cells' application boundary for validating an optional Fleet
/// node-pool selection. Implementations live in an outer anti-corruption
/// adapter and return no Fleet model or placement state.
#[async_trait]
pub trait IDurableCellNodePoolPort: Send + Sync {
    async fn validate_selection(
        &self,
        request: &DurableCellNodePoolSelectionRequest,
    ) -> ApplicationResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn selection_request_accepts_an_optional_pool() {
        let request = DurableCellNodePoolSelectionRequest::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            Some(NodePoolId::new()),
        );
        request.validate().expect("valid node-pool selection");
    }

    #[test]
    fn selection_request_rejects_a_nil_pool() {
        let request = DurableCellNodePoolSelectionRequest::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            Some(NodePoolId::from_uuid(Uuid::nil())),
        );
        assert!(request.validate().is_err());
    }
}
