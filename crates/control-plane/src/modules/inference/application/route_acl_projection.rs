//! Narrow Inference owner query for Edge Gateway snapshot ACL projection.
//!
//! Edge receives only contract-level route/grant projections. Catalog entities,
//! scheduling, and Power worker observations stay private to Inference (+ Power).

use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::InferenceRouteAclProjection;
use async_trait::async_trait;

/// Exact Inference-owned environment scope used to load route ACL projections
/// for one managed Gateway snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InferenceRouteEnvironmentScope {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
}

impl InferenceRouteEnvironmentScope {
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Self, String> {
        if organization_id.as_uuid().is_nil()
            || project_id.as_uuid().is_nil()
            || environment_id.as_uuid().is_nil()
        {
            return Err("inference route environment scope requires non-nil identities".into());
        }
        Ok(Self {
            organization_id,
            project_id,
            environment_id,
        })
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(self) -> ProjectId {
        self.project_id
    }

    pub const fn environment_id(self) -> EnvironmentId {
        self.environment_id
    }
}

/// Inference-owned projection port for managed Gateway inference route ACL.
///
/// Until durable catalog/route authority lands, production may wire an empty
/// adapter. Edge must never invent catalog or worker facts.
#[async_trait]
pub trait IInferenceRouteAclProjectionPort: Send + Sync {
    async fn list_inference_route_acl_projections(
        &self,
        scopes: &[InferenceRouteEnvironmentScope],
    ) -> Result<Vec<InferenceRouteAclProjection>, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn scope_rejects_nil_identities() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        assert!(InferenceRouteEnvironmentScope::new(
            organization_id,
            project_id,
            environment_id
        )
        .is_ok());
        assert!(InferenceRouteEnvironmentScope::new(
            OrganizationId::from_uuid(Uuid::nil()),
            project_id,
            environment_id,
        )
        .is_err());
    }
}
