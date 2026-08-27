use crate::modules::shared_kernel::domain::OrganizationId;
use crate::modules::sources::domain::{
    GitProvider, GitRepository, GithubConnection, GithubInstallationTokenRequest,
    IGithubConnectionRepository, IGithubInstallationTokenService, SourceProviderCredential,
};
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRepositoryCredentialRequest {
    pub organization_id: OrganizationId,
    pub repository: GitRepository,
}

impl SourceRepositoryCredentialRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.repository.provider() != GitProvider::Github
            || GitRepository::parse(self.repository.provider(), self.repository.canonical_url())?
                != self.repository
        {
            return Err("source repository credential request is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceRepositoryCredentialError {
    #[error("source repository credential request is invalid: {0}")]
    Invalid(String),
    #[error("source repository credential is unavailable")]
    Unavailable,
    #[error("source repository credential authority failed integrity validation: {0}")]
    Integrity(String),
    #[error("source repository credential storage failed: {0}")]
    Storage(String),
}

/// Sources-owned provider-credential authority for one canonical repository.
///
/// Consumers may request an ephemeral credential only after their anonymous
/// provider operation failed. Connection selection, aggregate restoration,
/// credential-request construction, and error redaction remain here; the
/// existing Domain port retains current installation revalidation and token
/// issuance.
#[async_trait]
pub trait ISourceRepositoryCredentialProvider: Send + Sync {
    async fn issue(
        &self,
        request: &SourceRepositoryCredentialRequest,
    ) -> Result<SourceProviderCredential, SourceRepositoryCredentialError>;
}

pub struct SourceRepositoryCredentialService {
    connections: Arc<dyn IGithubConnectionRepository>,
    installation_tokens: Arc<dyn IGithubInstallationTokenService>,
}

impl SourceRepositoryCredentialService {
    pub fn new(
        connections: Arc<dyn IGithubConnectionRepository>,
        installation_tokens: Arc<dyn IGithubInstallationTokenService>,
    ) -> Self {
        Self {
            connections,
            installation_tokens,
        }
    }
}

#[async_trait]
impl ISourceRepositoryCredentialProvider for SourceRepositoryCredentialService {
    async fn issue(
        &self,
        request: &SourceRepositoryCredentialRequest,
    ) -> Result<SourceProviderCredential, SourceRepositoryCredentialError> {
        request
            .validate()
            .map_err(SourceRepositoryCredentialError::Invalid)?;
        let connection = self
            .connections
            .find(request.organization_id)
            .await
            .map_err(|_| {
                SourceRepositoryCredentialError::Storage("source connection lookup failed".into())
            })?
            .ok_or(SourceRepositoryCredentialError::Unavailable)?;
        let connection = GithubConnection::restore(connection).map_err(|_| {
            SourceRepositoryCredentialError::Integrity(
                "stored source connection failed integrity validation".into(),
            )
        })?;
        if connection.organization_id != request.organization_id || !connection.is_authoritative() {
            return Err(SourceRepositoryCredentialError::Unavailable);
        }
        self.installation_tokens
            .issue(GithubInstallationTokenRequest {
                organization_id: request.organization_id,
                connection_id: connection.id,
                installation_id: connection.installation_id,
                repository: request.repository.clone(),
                requested_at: chrono::Utc::now(),
            })
            .await
            .map_err(|_| SourceRepositoryCredentialError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn credential_request_requires_exact_canonical_scope() {
        let repository =
            GitRepository::parse(GitProvider::Github, "https://github.com/A3S-Lab/Cloud.git/")
                .expect("canonicalizable repository");
        let valid = SourceRepositoryCredentialRequest {
            organization_id: OrganizationId::new(),
            repository: repository.clone(),
        };
        assert!(valid.validate().is_ok());
        assert_eq!(
            repository.canonical_url(),
            "https://github.com/a3s-lab/cloud"
        );

        let invalid = SourceRepositoryCredentialRequest {
            organization_id: OrganizationId::from_uuid(Uuid::nil()),
            repository,
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn credential_service_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SourceRepositoryCredentialService>();
    }
}
