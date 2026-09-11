use crate::modules::edge::domain::Route;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::{
    InferenceCredentialAclProjection, InferenceRouteAclProjection, InferenceWorkerAclProjection,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Exact environment identity Edge may add when compiling managed inference ACL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EdgeManagedInferenceAclEnvironment {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
}

impl EdgeManagedInferenceAclEnvironment {
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Self, String> {
        let scope = Self {
            organization_id,
            project_id,
            environment_id,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
        {
            return Err(
                "Edge managed inference ACL environment requires non-nil organization, project, and environment"
                    .into(),
            );
        }
        Ok(())
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

/// Edge-owned aggregate of managed Gateway inference ACL projections.
///
/// Values are Published Language contract projections. Identity and Inference
/// remain the projection authorities; Edge compiles them into Gateway snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeManagedInferenceAclSnapshot {
    pub credentials: Vec<InferenceCredentialAclProjection>,
    pub routes: Vec<InferenceRouteAclProjection>,
    pub workers: Vec<InferenceWorkerAclProjection>,
}

/// Edge-owned read port for managed Gateway inference ACL staging.
#[async_trait]
pub trait IEdgeManagedInferenceAclAccess: Send + Sync {
    async fn load_for_routes(
        &self,
        routes: &[Route],
        additional_environments: &[EdgeManagedInferenceAclEnvironment],
        projected_at: DateTime<Utc>,
    ) -> Result<EdgeManagedInferenceAclSnapshot, RepositoryError>;
}
