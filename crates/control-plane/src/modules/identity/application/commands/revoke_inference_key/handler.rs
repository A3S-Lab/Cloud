use super::RevokeInferenceKey;
use crate::modules::identity::application::InferenceCredentialMutationResult;
use crate::modules::identity::domain::events::InferenceCredentialChanged;
use crate::modules::identity::domain::repositories::{
    IInferenceCredentialLifecycleRepository, RevokeInferenceCredentialWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use serde::Serialize;
use std::sync::Arc;

pub struct RevokeInferenceKeyHandler {
    credentials: Arc<dyn IInferenceCredentialLifecycleRepository>,
}

impl RevokeInferenceKeyHandler {
    pub fn new(credentials: Arc<dyn IInferenceCredentialLifecycleRepository>) -> Self {
        Self { credentials }
    }
}

impl CommandHandler<RevokeInferenceKey> for RevokeInferenceKeyHandler {
    fn execute(
        &self,
        command: RevokeInferenceKey,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<InferenceCredentialMutationResult>>,
    > {
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            if command.expected_aggregate_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected inference credential aggregate version must be positive".into(),
                )));
            }
            let canonical = serde_json::to_vec(&CanonicalRevokeInferenceKey {
                organization_id: command.organization_id,
                credential_id: command.credential_id,
                expected_aggregate_version: command.expected_aggregate_version,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/inference/keys/{}/revoke",
                    command.organization_id, command.credential_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match credentials
                .replay_inference_credential_write(command.organization_id, &idempotency)
                .await
            {
                Ok(Some(write)) => {
                    return Ok(Ok(InferenceCredentialMutationResult {
                        credential: write.credential,
                        replayed: true,
                    }))
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let mut credential = match credentials
                .find_inference_credential(command.organization_id, command.credential_id)
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "inference key not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if credential.aggregate_version() != command.expected_aggregate_version {
                return Ok(Err(ApplicationError::Conflict(
                    "inference key changed before revocation".into(),
                )));
            }
            let changed = match credential.revoke(command.requested_at) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = if changed {
                Some(
                    InferenceCredentialChanged::revoked(&credential, command.request_id)
                        .map_err(|error| BootError::Internal(error.to_string()))?,
                )
            } else {
                None
            };
            let write = match credentials
                .revoke_inference_credential(RevokeInferenceCredentialWrite {
                    credential,
                    expected_aggregate_version: command.expected_aggregate_version,
                    idempotency,
                    request_id: command.request_id,
                    event,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(InferenceCredentialMutationResult {
                credential: write.credential,
                replayed: write.replayed,
            }))
        })
    }
}

#[derive(Serialize)]
struct CanonicalRevokeInferenceKey {
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    credential_id: crate::modules::shared_kernel::domain::InferenceCredentialId,
    expected_aggregate_version: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::application::commands::create_inference_key::{
        CreateInferenceKey, CreateInferenceKeyHandler,
    };
    use crate::modules::identity::domain::repositories::IInferenceCredentialRepository;
    use crate::modules::identity::infrastructure::persistence::InMemoryInferenceCredentialRepository;
    use crate::modules::identity::infrastructure::InferenceCredentialIssuer;
    use crate::modules::projects::domain::entities::Environment;
    use crate::modules::projects::domain::repositories::IEnvironmentRepository;
    use crate::modules::projects::domain::value_objects::EnvironmentName;
    use crate::modules::secrets::domain::{
        EncryptedSecretValue, ISecretEncryptionService, SecretEncryptionError,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
    };
    use a3s_boot::{CommandHandler, ModuleRef};
    use a3s_cloud_contracts::DomainEventEnvelope;
    use async_trait::async_trait;
    use base64::engine::general_purpose::STANDARD_NO_PAD;
    use base64::Engine as _;
    use chrono::{Duration, Utc};
    use sha2::{Digest, Sha256};
    use std::sync::Arc;
    use uuid::Uuid;

    struct AlwaysPresentEnvironmentRepository;

    #[async_trait]
    impl IEnvironmentRepository for AlwaysPresentEnvironmentRepository {
        async fn create(
            &self,
            environment: Environment,
            _event: DomainEventEnvelope,
            _idempotency: crate::modules::shared_kernel::domain::IdempotencyRequest,
        ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
            Ok(IdempotentWrite {
                value: environment,
                replayed: false,
            })
        }

        async fn find(
            &self,
            organization_id: OrganizationId,
            project_id: ProjectId,
            environment_id: EnvironmentId,
        ) -> Result<Option<Environment>, RepositoryError> {
            Ok(Some(Environment::create(
                organization_id,
                project_id,
                environment_id,
                EnvironmentName::parse("default").expect("environment name"),
                Utc::now(),
            )))
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
        ) -> Result<Vec<Environment>, RepositoryError> {
            Ok(Vec::new())
        }
    }

    struct TestEncryption;

    #[async_trait]
    impl ISecretEncryptionService for TestEncryption {
        async fn encrypt(
            &self,
            plaintext: &[u8],
            context: &[u8],
        ) -> Result<EncryptedSecretValue, SecretEncryptionError> {
            let context_digest = format!("{:x}", Sha256::digest(context));
            EncryptedSecretValue::new(
                "test:base64",
                format!("v1:{context_digest}:{}", STANDARD_NO_PAD.encode(plaintext)),
            )
            .map_err(SecretEncryptionError::Rejected)
        }

        async fn decrypt(
            &self,
            value: &EncryptedSecretValue,
            context: &[u8],
        ) -> Result<Vec<u8>, SecretEncryptionError> {
            let mut parts = value.ciphertext().splitn(3, ':');
            let version = parts.next();
            let context_digest = parts.next();
            let encoded = parts.next();
            let expected_context_digest = format!("{:x}", Sha256::digest(context));
            if version != Some("v1") || context_digest != Some(expected_context_digest.as_str()) {
                return Err(SecretEncryptionError::Rejected(
                    "test ciphertext context mismatch".into(),
                ));
            }
            STANDARD_NO_PAD
                .decode(encoded.unwrap_or_default())
                .map_err(|error| SecretEncryptionError::Rejected(error.to_string()))
        }

        async fn health(&self) -> Result<bool, SecretEncryptionError> {
            Ok(true)
        }
    }

    fn context() -> CqrsContext {
        CqrsContext::new(ModuleRef::new())
    }

    async fn create_active_key(
        credentials: &Arc<InMemoryInferenceCredentialRepository>,
    ) -> crate::modules::identity::domain::entities::InferenceCredential {
        let requested_at = Utc::now();
        let created = CreateInferenceKeyHandler::new(
            Arc::new(AlwaysPresentEnvironmentRepository),
            Arc::clone(credentials)
                as Arc<dyn crate::modules::identity::domain::repositories::IInferenceCredentialLifecycleRepository>,
            InferenceCredentialIssuer::new(),
            Arc::new(TestEncryption),
        )
        .execute(
            CreateInferenceKey {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
                expires_at: requested_at + Duration::hours(1),
                idempotency_key: format!("seed-{}", Uuid::now_v7()),
                request_id: Uuid::now_v7(),
                requested_at,
            },
            context(),
        )
        .await
        .expect("boot")
        .expect("create");
        created.credential
    }

    #[tokio::test]
    async fn revoke_marks_gateway_projection_revoked() {
        let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
        let credential = create_active_key(&credentials).await;
        let revoked = RevokeInferenceKeyHandler::new(
            Arc::clone(&credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>
        )
        .execute(
            RevokeInferenceKey {
                organization_id: credential.organization_id,
                credential_id: credential.id,
                expected_aggregate_version: credential.aggregate_version(),
                idempotency_key: "revoke-once".into(),
                request_id: Uuid::now_v7(),
                requested_at: credential.updated_at() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .expect("boot")
        .expect("revoke");

        assert!(
            revoked
                .credential
                .gateway_projection()
                .expect("projection")
                .revoked,
            "gateway ACL projection must flip to revoked after RevokeInferenceKey"
        );
        assert!(!revoked.replayed);
        let listed = credentials
            .list_inference_credentials_by_environment(
                credential.organization_id,
                credential.project_id,
                credential.environment_id,
            )
            .await
            .expect("list");
        assert_eq!(listed.len(), 1);
        assert!(listed[0].revoked_at().is_some());
    }

    #[tokio::test]
    async fn revoke_idempotent_replay_returns_same_revoked_credential() {
        let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
        let credential = create_active_key(&credentials).await;
        let command = RevokeInferenceKey {
            organization_id: credential.organization_id,
            credential_id: credential.id,
            expected_aggregate_version: credential.aggregate_version(),
            idempotency_key: "revoke-replay".into(),
            request_id: Uuid::now_v7(),
            requested_at: credential.updated_at() + Duration::seconds(1),
        };
        let handler = RevokeInferenceKeyHandler::new(
            Arc::clone(&credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>
        );
        let first = handler
            .execute(command.clone(), context())
            .await
            .expect("boot")
            .expect("first revoke");
        let replay = handler
            .execute(command, context())
            .await
            .expect("boot")
            .expect("replay");

        assert_eq!(replay.credential.id, first.credential.id);
        assert_eq!(
            replay.credential.aggregate_version(),
            first.credential.aggregate_version()
        );
        assert!(replay.replayed);
        assert!(
            replay
                .credential
                .gateway_projection()
                .expect("projection")
                .revoked
        );
    }
}
