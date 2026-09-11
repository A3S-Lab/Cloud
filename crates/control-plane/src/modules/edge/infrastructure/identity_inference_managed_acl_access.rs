use crate::modules::edge::application::{
    EdgeManagedInferenceAclEnvironment, EdgeManagedInferenceAclSnapshot,
    IEdgeManagedInferenceAclAccess,
};
use crate::modules::edge::domain::Route;
use crate::modules::edge::infrastructure::inference_credential_scope::inference_credential_scopes_from_routes;
use crate::modules::edge::infrastructure::inference_route_scope::inference_route_scopes_from_routes;
use crate::modules::identity::application::{
    IInferenceCredentialAclProjectionPort, InferenceCredentialEnvironmentScope,
};
use crate::modules::inference::application::{
    IInferenceRouteAclProjectionPort, IInferenceWorkerAclProjectionPort,
    InferenceRouteEnvironmentScope,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Anti-corruption adapter that loads Identity/Inference ACL projections for
/// managed Gateway snapshot staging. It creates no projection authority.
#[derive(Clone)]
pub struct IdentityInferenceEdgeManagedAclAccessAdapter {
    credentials: Arc<dyn IInferenceCredentialAclProjectionPort>,
    routes: Arc<dyn IInferenceRouteAclProjectionPort>,
    workers: Arc<dyn IInferenceWorkerAclProjectionPort>,
}

impl IdentityInferenceEdgeManagedAclAccessAdapter {
    pub fn new(
        credentials: Arc<dyn IInferenceCredentialAclProjectionPort>,
        routes: Arc<dyn IInferenceRouteAclProjectionPort>,
        workers: Arc<dyn IInferenceWorkerAclProjectionPort>,
    ) -> Self {
        Self {
            credentials,
            routes,
            workers,
        }
    }
}

#[async_trait]
impl IEdgeManagedInferenceAclAccess for IdentityInferenceEdgeManagedAclAccessAdapter {
    async fn load_for_routes(
        &self,
        routes: &[Route],
        additional_environments: &[EdgeManagedInferenceAclEnvironment],
        projected_at: DateTime<Utc>,
    ) -> Result<EdgeManagedInferenceAclSnapshot, RepositoryError> {
        let mut credential_scopes =
            inference_credential_scopes_from_routes(routes).map_err(RepositoryError::Conflict)?;
        let mut route_scopes =
            inference_route_scopes_from_routes(routes).map_err(RepositoryError::Conflict)?;
        for environment in additional_environments {
            environment.validate().map_err(RepositoryError::Conflict)?;
            let credential_scope = InferenceCredentialEnvironmentScope::new(
                environment.organization_id(),
                environment.project_id(),
                environment.environment_id(),
            )
            .map_err(RepositoryError::Conflict)?;
            if !credential_scopes.contains(&credential_scope) {
                credential_scopes.push(credential_scope);
            }
            let route_scope = InferenceRouteEnvironmentScope::new(
                environment.organization_id(),
                environment.project_id(),
                environment.environment_id(),
            )
            .map_err(RepositoryError::Conflict)?;
            if !route_scopes.contains(&route_scope) {
                route_scopes.push(route_scope);
            }
        }
        credential_scopes.sort();
        route_scopes.sort();
        let credentials = self
            .credentials
            .list_inference_credential_acl_projections(&credential_scopes)
            .await?;
        let inference_routes = self
            .routes
            .list_inference_route_acl_projections(&route_scopes)
            .await?;
        let workers = self
            .workers
            .list_inference_worker_acl_projections(&route_scopes, projected_at)
            .await?;
        Ok(EdgeManagedInferenceAclSnapshot {
            credentials,
            routes: inference_routes,
            workers,
        })
    }
}
