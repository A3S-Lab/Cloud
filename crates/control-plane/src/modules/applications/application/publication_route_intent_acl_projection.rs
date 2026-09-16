//! Applications-owned ACL projection port for Edge Gateway snapshot compile.
//!
//! `APP0.3-C16` exposes C15 Edge projections as Published Language contract
//! projections. Edge consumes this port through one ACA adapter and must not
//! import Applications aggregates.

use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId, RepositoryError};
use a3s_cloud_contracts::ApplicationPublicationRouteIntentAclProjection;
use async_trait::async_trait;

/// Exact Applications-owned org/project scope used to load publication route
/// intent ACL projections for one managed Gateway snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApplicationPublicationRouteIntentAclScope {
    organization_id: OrganizationId,
    project_id: ProjectId,
}

impl ApplicationPublicationRouteIntentAclScope {
    pub fn new(organization_id: OrganizationId, project_id: ProjectId) -> Result<Self, String> {
        if organization_id.as_uuid().is_nil() || project_id.as_uuid().is_nil() {
            return Err(
                "application publication route intent ACL scope requires non-nil identities".into(),
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

/// Applications-owned projection port for managed Gateway publication route intent ACL.
#[async_trait]
pub trait IApplicationPublicationRouteIntentAclProjectionPort: Send + Sync {
    async fn list_publication_route_intent_acl_projections(
        &self,
        scopes: &[ApplicationPublicationRouteIntentAclScope],
    ) -> Result<Vec<ApplicationPublicationRouteIntentAclProjection>, RepositoryError>;
}
