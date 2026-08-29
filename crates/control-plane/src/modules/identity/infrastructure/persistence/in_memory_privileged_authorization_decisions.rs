use super::InMemoryIdentityRepository;
use crate::modules::identity::domain::repositories::IPrivilegedAuthorizationDecisionRepository;
use crate::modules::identity::domain::services::PrivilegedAuthorizationDecisionRequest;
use crate::modules::shared_kernel::domain::{AuthorizationDecisionRef, RepositoryError};
use async_trait::async_trait;

/// The in-memory Identity adapter intentionally has no platform-policy or
/// support-grant authority. Tests must supply an explicit decision fake when
/// they exercise privileged access.
#[async_trait]
impl IPrivilegedAuthorizationDecisionRepository for InMemoryIdentityRepository {
    async fn authorize_privileged(
        &self,
        _request: PrivilegedAuthorizationDecisionRequest,
    ) -> Result<AuthorizationDecisionRef, RepositoryError> {
        Err(RepositoryError::Forbidden(
            "privileged authorization requires the PostgreSQL Identity authority".into(),
        ))
    }
}
