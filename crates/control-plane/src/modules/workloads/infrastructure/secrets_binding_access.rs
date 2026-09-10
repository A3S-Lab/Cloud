use crate::modules::secrets::domain::ISecretRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::application::{
    IWorkloadsSecretBindingAccess, WorkloadsSecretBindingScope,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Read-only anti-corruption adapter for the Secrets binding-admission authority.
#[derive(Clone)]
pub struct SecretsWorkloadsSecretBindingAccessAdapter {
    secrets: Arc<dyn ISecretRepository>,
}

impl SecretsWorkloadsSecretBindingAccessAdapter {
    pub fn new(secrets: Arc<dyn ISecretRepository>) -> Self {
        Self { secrets }
    }
}

#[async_trait]
impl IWorkloadsSecretBindingAccess for SecretsWorkloadsSecretBindingAccessAdapter {
    async fn binding_is_admissible(
        &self,
        scope: WorkloadsSecretBindingScope,
    ) -> Result<bool, RepositoryError> {
        scope.validate().map_err(RepositoryError::Forbidden)?;
        let secret = match self
            .secrets
            .find(scope.organization_id(), scope.secret_id())
            .await
        {
            Ok(secret)
                if secret.organization_id == scope.organization_id()
                    && secret.id == scope.secret_id() =>
            {
                secret
            }
            Ok(_) => {
                return Err(RepositoryError::Storage(
                    "Secrets returned inconsistent Workloads binding evidence".into(),
                ))
            }
            Err(RepositoryError::NotFound) => return Ok(false),
            Err(error) => return Err(error),
        };
        if secret.project_id != scope.project_id()
            || secret.environment_id != scope.environment_id()
        {
            return Ok(false);
        }
        let version = match self
            .secrets
            .find_version(scope.organization_id(), scope.secret_id(), scope.version())
            .await
        {
            Ok(version)
                if version.secret_id == scope.secret_id() && version.version == scope.version() =>
            {
                version
            }
            Ok(_) => {
                return Err(RepositoryError::Storage(
                    "Secrets returned inconsistent Workloads binding version evidence".into(),
                ))
            }
            Err(RepositoryError::NotFound) => return Ok(false),
            Err(error) => return Err(error),
        };
        Ok(version.is_materializable(&secret))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::secrets::domain::{
        CreateSecretWrite, EncryptedSecretValue, RotateSecretWrite, Secret, SecretVersion,
        SecretWrite, TransitionSecretVersion,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, IdempotencyRequest, OrganizationId, ProjectId, ResourceName, SecretId,
    };
    use chrono::Utc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct StubSecretRepository {
        secret: Option<Secret>,
        version: Option<SecretVersion>,
        find_calls: AtomicUsize,
        find_version_calls: AtomicUsize,
    }

    #[async_trait]
    impl ISecretRepository for StubSecretRepository {
        async fn replay_write(
            &self,
            _organization_id: OrganizationId,
            _idempotency: &IdempotencyRequest,
        ) -> Result<Option<SecretWrite>, RepositoryError> {
            unreachable!("binding access adapter never replays writes")
        }

        async fn create(&self, _bundle: CreateSecretWrite) -> Result<SecretWrite, RepositoryError> {
            unreachable!("binding access adapter never creates secrets")
        }

        async fn rotate(&self, _bundle: RotateSecretWrite) -> Result<SecretWrite, RepositoryError> {
            unreachable!("binding access adapter never rotates secrets")
        }

        async fn transition_version(
            &self,
            _bundle: TransitionSecretVersion,
        ) -> Result<SecretWrite, RepositoryError> {
            unreachable!("binding access adapter never transitions versions")
        }

        async fn find(
            &self,
            organization_id: OrganizationId,
            secret_id: SecretId,
        ) -> Result<Secret, RepositoryError> {
            self.find_calls.fetch_add(1, Ordering::SeqCst);
            match &self.secret {
                Some(secret)
                    if secret.organization_id == organization_id && secret.id == secret_id =>
                {
                    Ok(secret.clone())
                }
                Some(_) | None => Err(RepositoryError::NotFound),
            }
        }

        async fn find_version(
            &self,
            _organization_id: OrganizationId,
            secret_id: SecretId,
            version: u64,
        ) -> Result<SecretVersion, RepositoryError> {
            self.find_version_calls.fetch_add(1, Ordering::SeqCst);
            match &self.version {
                Some(secret_version)
                    if secret_version.secret_id == secret_id
                        && secret_version.version == version =>
                {
                    Ok(secret_version.clone())
                }
                Some(_) | None => Err(RepositoryError::NotFound),
            }
        }

        async fn find_materializable_version(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
            _secret_id: SecretId,
            _version: u64,
        ) -> Result<SecretVersion, RepositoryError> {
            unreachable!("binding access adapter never uses materializable lookup")
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
        ) -> Result<Vec<Secret>, RepositoryError> {
            unreachable!("binding access adapter never lists secrets")
        }

        async fn list_versions(
            &self,
            _organization_id: OrganizationId,
            _secret_id: SecretId,
        ) -> Result<Vec<SecretVersion>, RepositoryError> {
            unreachable!("binding access adapter never lists versions")
        }
    }

    #[tokio::test]
    async fn adapter_projects_only_exact_admissible_binding_evidence() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let secret_id = SecretId::new();
        let (secret, version) = Secret::create(
            secret_id,
            organization_id,
            project_id,
            environment_id,
            ResourceName::parse("database-password").expect("secret name"),
            EncryptedSecretValue::new("test-key", "ciphertext").expect("encrypted value"),
            Utc::now(),
        )
        .expect("secret");
        let present =
            SecretsWorkloadsSecretBindingAccessAdapter::new(Arc::new(StubSecretRepository {
                secret: Some(secret),
                version: Some(version),
                find_calls: AtomicUsize::new(0),
                find_version_calls: AtomicUsize::new(0),
            }));
        let scope = WorkloadsSecretBindingScope::new(
            organization_id,
            project_id,
            environment_id,
            secret_id,
            1,
        )
        .unwrap();
        assert!(present.binding_is_admissible(scope).await.unwrap());

        let missing =
            SecretsWorkloadsSecretBindingAccessAdapter::new(Arc::new(StubSecretRepository {
                secret: None,
                version: None,
                find_calls: AtomicUsize::new(0),
                find_version_calls: AtomicUsize::new(0),
            }));
        assert!(!missing.binding_is_admissible(scope).await.unwrap());

        let wrong_environment =
            SecretsWorkloadsSecretBindingAccessAdapter::new(Arc::new(StubSecretRepository {
                secret: Some(
                    Secret::create(
                        secret_id,
                        organization_id,
                        project_id,
                        EnvironmentId::new(),
                        ResourceName::parse("database-password").expect("secret name"),
                        EncryptedSecretValue::new("test-key", "ciphertext")
                            .expect("encrypted value"),
                        Utc::now(),
                    )
                    .expect("secret")
                    .0,
                ),
                version: None,
                find_calls: AtomicUsize::new(0),
                find_version_calls: AtomicUsize::new(0),
            }));
        assert!(!wrong_environment
            .binding_is_admissible(scope)
            .await
            .unwrap());
    }
}
