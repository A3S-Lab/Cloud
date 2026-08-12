use crate::modules::identity::domain::services::ResourceAuthorizationDecisionRequest;
use crate::modules::shared_kernel::domain::{AuthorizationDecisionRef, RepositoryError};
use async_trait::async_trait;

/// Resolves current Identity authority and persists its evidence in the existing audit store.
#[async_trait]
pub trait IResourceAuthorizationDecisionRepository: Send + Sync {
    async fn authorize_resource(
        &self,
        request: ResourceAuthorizationDecisionRequest,
    ) -> Result<AuthorizationDecisionRef, RepositoryError>;
}
