use crate::modules::identity::domain::services::PrivilegedAuthorizationDecisionRequest;
use crate::modules::shared_kernel::domain::{AuthorizationDecisionRef, RepositoryError};
use async_trait::async_trait;

/// Resolves one current Installation authority snapshot and records the exact
/// allow evidence through Identity's shared audit transaction.
#[async_trait]
pub trait IPrivilegedAuthorizationDecisionRepository: Send + Sync {
    async fn authorize_privileged(
        &self,
        request: PrivilegedAuthorizationDecisionRequest,
    ) -> Result<AuthorizationDecisionRef, RepositoryError>;
}
