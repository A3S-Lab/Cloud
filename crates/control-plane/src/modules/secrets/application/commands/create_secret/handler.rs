use super::CreateSecret;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::secrets::application::{encryption_error, SecretMutationResult};
use crate::modules::secrets::domain::{
    secret_encryption_context, CreateSecretWrite, ISecretEncryptionService, ISecretRepository,
    Secret, SecretChanged,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, ResourceName, SecretId};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;

pub struct CreateSecretHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    secrets: Arc<dyn ISecretRepository>,
    encryption: Arc<dyn ISecretEncryptionService>,
}

impl CreateSecretHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
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
            match environments
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found in organization and project".into(),
                    )))
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
