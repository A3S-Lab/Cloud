//! Adapts Inference route catalog storage into Edge ACL projections.

use crate::modules::inference::application::{
    IInferenceRouteAclProjectionPort, InferenceRouteEnvironmentScope,
};
use crate::modules::inference::domain::repositories::IInferenceRouteRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::InferenceRouteAclProjection;
use async_trait::async_trait;
use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Clone)]
pub struct InferenceRouteAclProjectionAdapter {
    routes: Arc<dyn IInferenceRouteRepository>,
}

impl InferenceRouteAclProjectionAdapter {
    pub fn new(routes: Arc<dyn IInferenceRouteRepository>) -> Self {
        Self { routes }
    }
}

#[async_trait]
impl IInferenceRouteAclProjectionPort for InferenceRouteAclProjectionAdapter {
    async fn list_inference_route_acl_projections(
        &self,
        scopes: &[InferenceRouteEnvironmentScope],
    ) -> Result<Vec<InferenceRouteAclProjection>, RepositoryError> {
        let unique = scopes.iter().copied().collect::<BTreeSet<_>>();
        let mut projections = Vec::new();
        for scope in unique {
            let routes = self
                .routes
                .list_active_inference_routes_by_environment(
                    scope.organization_id(),
                    scope.project_id(),
                    scope.environment_id(),
                )
                .await?;
            for route in routes {
                projections.push(
                    route
                        .gateway_projection()
                        .map_err(RepositoryError::Storage)?,
                );
            }
        }
        projections.sort_by_key(|projection| projection.route_id);
        Ok(projections)
    }
}
