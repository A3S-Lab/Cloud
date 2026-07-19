use super::RevokeSecretVersion;
use crate::modules::secrets::application::{SecretMutationResult, SecretVersionResult};
use crate::modules::secrets::domain::{
    ISecretRepository, SecretChanged, SecretVersionState, TransitionSecretVersion,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct RevokeSecretVersionHandler {
    secrets: Arc<dyn ISecretRepository>,
}

impl RevokeSecretVersionHandler {
    pub fn new(secrets: Arc<dyn ISecretRepository>) -> Self {
        Self { secrets }
    }
}

impl CommandHandler<RevokeSecretVersion> for RevokeSecretVersionHandler {
    fn execute(
        &self,
        command: RevokeSecretVersion,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<SecretMutationResult>>>
    {
        let secrets = Arc::clone(&self.secrets);
        Box::pin(async move {
            if command.version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "Secret version must be greater than zero".into(),
                )));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organization_id": command.organization_id,
                "secret_id": command.secret_id,
                "version": command.version,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/secrets/{}/versions/{}/revoke",
                    command.organization_id, command.secret_id, command.version
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
            let mut version = match secrets
                .find_version(command.organization_id, command.secret_id, command.version)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            if version.state == SecretVersionState::Revoked {
                return Ok(Ok(SecretMutationResult {
                    secret,
                    version: SecretVersionResult::from(&version),
                    replayed: true,
                }));
            }
            let expected_secret_version = secret.aggregate_version;
            let expected_version = version.aggregate_version;
            if let Err(error) = secret.revoke_version(&mut version, Utc::now()) {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            let event = SecretChanged::version_revoked(&secret, &version, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            Ok(secrets
                .transition_version(TransitionSecretVersion {
                    secret,
                    version,
                    expected_secret_version,
                    expected_version,
                    idempotency,
                    event,
                })
                .await
                .map(Into::into)
                .map_err(Into::into))
        })
    }
}
