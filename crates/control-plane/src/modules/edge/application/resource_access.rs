use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::{DomainClaim, Route};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, OrganizationId, ProjectId, RepositoryError, RouteId,
};
use std::collections::BTreeSet;
use std::sync::Arc;

/// One Edge visibility selector projected from an Identity decision.
///
/// Project grants cover every environment under that project. Environment grants
/// are exact. Node grants have no ownership meaning for route visibility and are
/// discarded by the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl EdgeAccessScope {
    fn allows_environment(self, project_id: ProjectId, environment_id: EnvironmentId) -> bool {
        match self {
            Self::Project {
                project_id: granted,
            } => granted == project_id,
            Self::Environment {
                project_id: granted_project,
                environment_id: granted_environment,
            } => granted_project == project_id && granted_environment == environment_id,
        }
    }
}

/// Edge-owned projection of an already-authorized request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<EdgeAccessScope>,
}

impl EdgeAccess {
    pub fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub fn restricted(granted_scopes: impl IntoIterator<Item = EdgeAccessScope>) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

    pub fn granted_scopes(&self) -> impl Iterator<Item = EdgeAccessScope> + '_ {
        self.granted_scopes.iter().copied()
    }

    /// Project grants cover all descendant environments; environment grants are exact.
    pub fn environment_is_visible(
        &self,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|scope| scope.allows_environment(project_id, environment_id))
    }
}

/// Resolves indirect Edge identifiers through the owning repository before authorization.
///
/// Identity owns grant semantics at the edge; Edge owns the canonical Route- and
/// DomainClaim-to-environment relationships. Missing and denied identifiers therefore
/// share one application-layer not-found contract without an Identity-owned index.
#[derive(Clone)]
pub(crate) struct EdgeResourceAccess {
    edge: Arc<dyn IEdgeRepository>,
}

impl EdgeResourceAccess {
    pub fn new(edge: Arc<dyn IEdgeRepository>) -> Self {
        Self { edge }
    }

    pub async fn route(
        &self,
        organization_id: OrganizationId,
        route_id: RouteId,
        access: &EdgeAccess,
    ) -> ApplicationResult<Route> {
        let route = self
            .edge
            .find_route(organization_id, route_id)
            .await
            .map_err(map_route_repository_error)?;
        if !access.environment_is_visible(route.project_id, route.environment_id) {
            return Err(route_not_found());
        }
        Ok(route)
    }

    pub async fn domain_claim(
        &self,
        organization_id: OrganizationId,
        claim_id: DomainClaimId,
        access: &EdgeAccess,
    ) -> ApplicationResult<DomainClaim> {
        let claim = self
            .edge
            .find_domain_claim(organization_id, claim_id)
            .await
            .map_err(map_domain_claim_repository_error)?;
        if !access.environment_is_visible(claim.project_id, claim.environment_id) {
            return Err(domain_claim_not_found());
        }
        Ok(claim)
    }
}

fn map_route_repository_error(error: RepositoryError) -> ApplicationError {
    match error {
        RepositoryError::NotFound => route_not_found(),
        error => error.into(),
    }
}

fn map_domain_claim_repository_error(error: RepositoryError) -> ApplicationError {
    match error {
        RepositoryError::NotFound => domain_claim_not_found(),
        error => error.into(),
    }
}

fn route_not_found() -> ApplicationError {
    ApplicationError::NotFound("route not found".into())
}

fn domain_claim_not_found() -> ApplicationError {
    ApplicationError::NotFound("domain claim not found".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organization_wide_sees_every_environment() {
        let access = EdgeAccess::organization_wide();
        assert!(access.is_organization_wide());
        assert!(access.environment_is_visible(ProjectId::new(), EnvironmentId::new()));
    }

    #[test]
    fn project_grant_covers_environments_while_environment_grant_is_exact() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let project_access = EdgeAccess::restricted([EdgeAccessScope::Project { project_id }]);
        assert!(project_access.environment_is_visible(project_id, environment_id));
        assert!(!project_access.environment_is_visible(ProjectId::new(), environment_id));

        let environment_access = EdgeAccess::restricted([EdgeAccessScope::Environment {
            project_id,
            environment_id,
        }]);
        assert!(environment_access.environment_is_visible(project_id, environment_id));
        assert!(!environment_access.environment_is_visible(project_id, EnvironmentId::new()));
    }
}
