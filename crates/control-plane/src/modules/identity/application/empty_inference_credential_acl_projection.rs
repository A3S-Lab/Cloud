//! Empty Identity projection port for tests and credential-free managed paths.

use super::{IInferenceCredentialAclProjectionPort, InferenceCredentialEnvironmentScope};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::InferenceCredentialAclProjection;
use async_trait::async_trait;

/// Projection port that always returns zero credentials.
///
/// Use this when a managed Gateway path must hold the Identity port but the
/// fixture or path intentionally has no inference credentials.
#[derive(Debug, Default, Clone, Copy)]
pub struct EmptyInferenceCredentialAclProjectionPort;

#[async_trait]
impl IInferenceCredentialAclProjectionPort for EmptyInferenceCredentialAclProjectionPort {
    async fn list_inference_credential_acl_projections(
        &self,
        _scopes: &[InferenceCredentialEnvironmentScope],
    ) -> Result<Vec<InferenceCredentialAclProjection>, RepositoryError> {
        Ok(Vec::new())
    }
}
