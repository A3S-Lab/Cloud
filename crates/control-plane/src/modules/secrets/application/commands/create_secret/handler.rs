use super::CreateSecret;
use crate::modules::secrets::application::{
    ISecretEnvironmentAccess, SecretEnvironmentScope, SecretMutationResult, encryption_error,
};
use crate::modules::secrets::domain::{
    CreateSecretWrite, ISecretEncryptionService, ISecretRepository, Secret, SecretChanged,
    secret_encryption_context,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, ResourceName, SecretId};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;

pub struct CreateSecretHandler {
    environments: Arc<dyn ISecretEnvironmentAccess>,
    secrets: Arc<dyn ISecretRepository>,
    encryption: Arc<dyn ISecretEncryptionService>,
}

impl CreateSecretHandler {
    pub fn new(
        environments: Arc<dyn ISecretEnvironmentAccess>,
        secrets: Arc<dyn ISecretRepository>,
        encryption: Arc<dyn ISecretEncryptionService>,
    ) -> Self {
        Self {
            environments,
            secrets,
            encryption,
        }
    }
}

impl CommandHandler<CreateSecret> for CreateSecretHandler {
    fn execute(
        &self,
        command: CreateSecret,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<SecretMutationResult>>>
    {
        let environments = Arc::clone(&self.environments);
        let secrets = Arc::clone(&self.secrets);
        let encryption = Arc::clone(&self.encryption);
        Box::pin(async move {
            if !command
                .access
                .environment_is_visible(command.project_id, command.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "environment not found in organization and project".into(),
                )));
            }
            let environment_scope = match SecretEnvironmentScope::new(
                command.organization_id,
                command.project_id,
                command.environment_id,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match environments.environment_exists(environment_scope).await {
                Ok(true) => {}
                Ok(false) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found in organization and project".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            }
            let name = match ResourceName::parse(command.name) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let value_digest = command.value.digest();
            let canonical = serde_json::to_vec(&CanonicalCreateSecret {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                name: name.as_str(),
                value_digest: &value_digest,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/secrets",
                    command.organization_id, command.project_id, command.environment_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match secrets
                .replay_write(command.organization_id, &idempotency)
                .await
            {
                Ok(Some(write)) => return Ok(Ok(write.into())),
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let secret_id = SecretId::new();
            let context = secret_encryption_context(command.organization_id, secret_id, 1)
                .map_err(BootError::Internal)?;
            let encrypted = match encryption.encrypt(command.value.as_bytes(), &context).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(encryption_error(error))),
            };
            let (secret, version) = match Secret::create(
                secret_id,
                command.organization_id,
                command.project_id,
                command.environment_id,
                name,
                encrypted,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = SecretChanged::created(&secret, &version, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            Ok(secrets
                .create(CreateSecretWrite {
                    secret,
                    version,
                    idempotency,
                    event,
                })
                .await
                .map(Into::into)
                .map_err(Into::into))
        })
    }
}

#[derive(Serialize)]
struct CanonicalCreateSecret<'a> {
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    project_id: crate::modules::shared_kernel::domain::ProjectId,
    environment_id: crate::modules::shared_kernel::domain::EnvironmentId,
    name: &'a str,
    value_digest: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::secrets::application::resource_access::{SecretAccess, SecretAccessScope};
    use crate::modules::secrets::application::SecretPlaintext;
    use crate::modules::secrets::domain::{EncryptedSecretValue, SecretEncryptionError};
    use crate::modules::secrets::infrastructure::InMemorySecretRepository;
    use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;
    use uuid::Uuid;

    struct AlwaysPresentEnvironmentAccess;

    #[async_trait]
    impl ISecretEnvironmentAccess for AlwaysPresentEnvironmentAccess {
        async fn environment_exists(
            &self,
            _scope: SecretEnvironmentScope,
        ) -> Result<bool, crate::modules::shared_kernel::domain::RepositoryError> {
            Ok(true)
        }
    }

    struct RejectEncryption;

    #[async_trait]
    impl ISecretEncryptionService for RejectEncryption {
        async fn encrypt(
            &self,
            _plaintext: &[u8],
            _context: &[u8],
        ) -> Result<EncryptedSecretValue, SecretEncryptionError> {
            Err(SecretEncryptionError::Unavailable(
                "encryption must not run for denied creates".into(),
            ))
        }

        async fn decrypt(
            &self,
            _ciphertext: &EncryptedSecretValue,
            _context: &[u8],
        ) -> Result<Vec<u8>, SecretEncryptionError> {
            Err(SecretEncryptionError::Unavailable(
                "decrypt must not run for denied creates".into(),
            ))
        }

        async fn health(&self) -> Result<bool, SecretEncryptionError> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn create_secret_fails_closed_before_creating_in_an_ungranted_environment() {
        let project_id = ProjectId::new();
        let handler = CreateSecretHandler::new(
            Arc::new(AlwaysPresentEnvironmentAccess),
            Arc::new(InMemorySecretRepository::new()),
            Arc::new(RejectEncryption),
        );
        let result = handler
            .execute(
                CreateSecret {
                    organization_id: OrganizationId::new(),
                    project_id,
                    environment_id: EnvironmentId::new(),
                    access: SecretAccess::restricted([SecretAccessScope::Environment {
                        project_id: ProjectId::new(),
                        environment_id: EnvironmentId::new(),
                    }]),
                    name: "denied-secret".into(),
                    value: SecretPlaintext::new(b"secret".to_vec()).expect("plaintext"),
                    idempotency_key: "deny-create".into(),
                    request_id: Uuid::now_v7(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert!(matches!(
            result,
            Err(ApplicationError::NotFound(message))
                if message == "environment not found in organization and project"
        ));
    }
}
