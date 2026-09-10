//! Empty Inference route ACL projection until catalog authority is published.

use super::{IInferenceRouteAclProjectionPort, InferenceRouteEnvironmentScope};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::InferenceRouteAclProjection;
use async_trait::async_trait;

/// Returns no route/grant projections. Preserves the grant-empty inference shell
/// when Edge compiles managed snapshots without catalog publication.
#[derive(Debug, Default, Clone, Copy)]
pub struct EmptyInferenceRouteAclProjectionPort;

#[async_trait]
impl IInferenceRouteAclProjectionPort for EmptyInferenceRouteAclProjectionPort {
    async fn list_inference_route_acl_projections(
        &self,
        _scopes: &[InferenceRouteEnvironmentScope],
    ) -> Result<Vec<InferenceRouteAclProjection>, RepositoryError> {
        Ok(Vec::new())
    }
}
