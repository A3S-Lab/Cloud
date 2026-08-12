use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::Route;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError, RouteId};
use std::sync::Arc;

/// Resolves indirect Edge identifiers through the owning repository before authorization.
///
/// Identity owns grant semantics; Edge owns the canonical Route-to-environment relationship.
/// Missing and denied identifiers therefore share one application-layer not-found contract
/// without an Identity-owned route index or a presentation-only authorization decision.
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
        evaluator: &ResourceAccessEvaluator,
    ) -> ApplicationResult<Route> {
        let route = self
            .edge
            .find_route(organization_id, route_id)
            .await
            .map_err(map_route_repository_error)?;
        if !evaluator.allows(ResourceGrantScope::Environment {
            project_id: route.project_id,
            environment_id: route.environment_id,
        }) {
            return Err(route_not_found());
        }
        Ok(route)
    }
}

fn map_route_repository_error(error: RepositoryError) -> ApplicationError {
    match error {
        RepositoryError::NotFound => route_not_found(),
        error => error.into(),
    }
}

fn route_not_found() -> ApplicationError {
    ApplicationError::NotFound("route not found".into())
}
