//! Edge-owned read port for managed Gateway publication route intent ACL.
//!
//! `APP0.3-C16` admits Applications-owned declare-only intent projections into
//! Gateway snapshot compile. Edge must not import Applications aggregates;
//! Applications remains the projection authority behind one ACA adapter.

use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId, RepositoryError};
use a3s_cloud_contracts::ApplicationPublicationRouteIntentAclProjection;
use async_trait::async_trait;

/// Exact org/project scope Edge may request when compiling publication route intent ACL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeManagedApplicationPublicationRouteIntentScope {
    organization_id: OrganizationId,
    project_id: ProjectId,
}

impl EdgeManagedApplicationPublicationRouteIntentScope {
    pub fn new(organization_id: OrganizationId, project_id: ProjectId) -> Result<Self, String> {
        if organization_id.as_uuid().is_nil() || project_id.as_uuid().is_nil() {
            return Err(
                "Edge managed publication route intent scope requires non-nil organization and project"
                    .into(),
            );
        }
        Ok(Self {
            organization_id,
            project_id,
        })
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(self) -> ProjectId {
        self.project_id
    }
}

/// Edge-owned read port for managed Gateway publication route intent ACL staging.
#[async_trait]
pub trait IEdgeManagedApplicationPublicationRouteIntentAccess: Send + Sync {
    async fn list_for_scopes(
        &self,
        scopes: &[EdgeManagedApplicationPublicationRouteIntentScope],
    ) -> Result<Vec<ApplicationPublicationRouteIntentAclProjection>, RepositoryError>;
}
