use super::RotateSecret;
use crate::modules::secrets::application::{encryption_error, SecretMutationResult};
use crate::modules::secrets::domain::{
    secret_encryption_context, ISecretEncryptionService, ISecretRepository, RotateSecretWrite,
    SecretChanged, SecretState,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;

pub struct RotateSecretHandler {
    secrets: Arc<dyn ISecretRepository>,
    encryption: Arc<dyn ISecretEncryptionService>,
}

impl RotateSecretHandler {
    pub fn new(
        secrets: Arc<dyn ISecretRepository>,
        encryption: Arc<dyn ISecretEncryptionService>,
    ) -> Self {
        Self {
            secrets,
            encryption,
        }
    }
}

impl CommandHandler<RotateSecret> for RotateSecretHandler {
    fn execute(
        &self,
        command: RotateSecret,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<SecretMutationResult>>>
    {
        let secrets = Arc::clone(&self.secrets);
        let encryption = Arc::clone(&self.encryption);
        Box::pin(async move {
            let value_digest = command.value.digest();
            let canonical = serde_json::to_vec(&CanonicalRotateSecret {
                organization_id: command.organization_id,
                secret_id: command.secret_id,
                value_digest: &value_digest,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/secrets/{}/versions",
                    command.organization_id, command.secret_id
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
            let mut secret = match secrets
                .find(command.organization_id, command.secret_id)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            if secret.state != SecretState::Active {
                return Ok(Err(ApplicationError::Conflict(
                    "revoked Secret cannot create another version".into(),
                )));
            }
            let expected_secret_version = secret.aggregate_version;
            let next_version = match secret.current_version.checked_add(1) {
                Some(value) => value,
                None => {
                    return Ok(Err(ApplicationError::Conflict(
                        "Secret version overflowed".into(),
                    )))
                }
            };
            let context =
                secret_encryption_context(command.organization_id, command.secret_id, next_version)
                    .map_err(BootError::Internal)?;
            let encrypted = match encryption.encrypt(command.value.as_bytes(), &context).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(encryption_error(error))),
            };
            let version = match secret.rotate(encrypted, Utc::now()) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let event = SecretChanged::rotated(&secret, &version, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            Ok(secrets
                .rotate(RotateSecretWrite {
                    secret,
                    version,
                    expected_secret_version,
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
struct CanonicalRotateSecret<'a> {
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    secret_id: crate::modules::shared_kernel::domain::SecretId,
    value_digest: &'a str,
}
