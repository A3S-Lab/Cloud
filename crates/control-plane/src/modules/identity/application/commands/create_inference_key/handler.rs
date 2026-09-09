use super::CreateInferenceKey;
use crate::modules::identity::application::{
    encrypt_inference_credential_delivery_receipt, recover_inference_credential_delivery,
    InferenceCredentialDeliveryResult,
};
use crate::modules::identity::domain::events::InferenceCredentialChanged;
use crate::modules::identity::domain::repositories::{
    CreateInferenceCredentialWrite, IInferenceCredentialLifecycleRepository,
};
use crate::modules::identity::infrastructure::{
    InferenceCredentialIssuanceError, InferenceCredentialIssueRequest, InferenceCredentialIssuer,
};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::secrets::domain::ISecretEncryptionService;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, RepositoryError};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use serde::Serialize;
use std::sync::Arc;

const MAX_IDENTITY_ATTEMPTS: usize = 4;

pub struct CreateInferenceKeyHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    credentials: Arc<dyn IInferenceCredentialLifecycleRepository>,
    issuer: InferenceCredentialIssuer,
    encryption: Arc<dyn ISecretEncryptionService>,
}

impl CreateInferenceKeyHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        credentials: Arc<dyn IInferenceCredentialLifecycleRepository>,
        issuer: InferenceCredentialIssuer,
        encryption: Arc<dyn ISecretEncryptionService>,
    ) -> Self {
        Self {
            environments,
            credentials,
            issuer,
            encryption,
        }
    }
}

impl CommandHandler<CreateInferenceKey> for CreateInferenceKeyHandler {
    fn execute(
        &self,
        command: CreateInferenceKey,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<InferenceCredentialDeliveryResult>>,
    > {
        let environments = Arc::clone(&self.environments);
        let credentials = Arc::clone(&self.credentials);
        let issuer = self.issuer.clone();
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
            let canonical = serde_json::to_vec(&CanonicalCreateInferenceKey {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                expires_at: command.expires_at,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/inference/keys",
                    command.organization_id, command.project_id, command.environment_id
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
                    return Ok(recover_inference_credential_delivery(
                        encryption.as_ref(),
                        write,
                        command.requested_at,
                    )
                    .await)
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }

            for attempt in 0..MAX_IDENTITY_ATTEMPTS {
                let issued = match issuer
                    .issue(InferenceCredentialIssueRequest {
                        organization_id: command.organization_id,
                        project_id: command.project_id,
                        environment_id: command.environment_id,
                        expires_at: command.expires_at,
                        issued_at: command.requested_at,
                    })
                    .await
                {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(issuance_error(error))),
                };
                let receipt = match encrypt_inference_credential_delivery_receipt(
                    encryption.as_ref(),
                    &issued.credential,
                    issued.secret.as_str(),
                )
                .await
                {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error)),
                };
                let event =
                    InferenceCredentialChanged::created(&issued.credential, command.request_id)
                        .map_err(|error| BootError::Internal(error.to_string()))?;
                match credentials
                    .create_inference_credential_delivery(CreateInferenceCredentialWrite {
                        credential: issued.credential,
                        receipt,
                        idempotency: idempotency.clone(),
                        event,
                    })
                    .await
                {
                    Ok(write) => {
                        return Ok(recover_inference_credential_delivery(
                            encryption.as_ref(),
                            write,
                            command.requested_at,
                        )
                        .await)
                    }
                    Err(error)
                        if identity_collision(&error) && attempt + 1 < MAX_IDENTITY_ATTEMPTS => {}
                    Err(error) if identity_collision(&error) => {
                        return Ok(Err(ApplicationError::Unavailable(
                            "inference key issuance exhausted its bounded identity retries".into(),
                        )))
                    }
                    Err(error) => return Ok(Err(error.into())),
                }
            }
            Ok(Err(ApplicationError::Unavailable(
                "inference key issuance is unavailable".into(),
            )))
        })
    }
}

#[derive(Serialize)]
struct CanonicalCreateInferenceKey {
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    project_id: crate::modules::shared_kernel::domain::ProjectId,
    environment_id: crate::modules::shared_kernel::domain::EnvironmentId,
    expires_at: chrono::DateTime<chrono::Utc>,
}

fn issuance_error(error: InferenceCredentialIssuanceError) -> ApplicationError {
    match error {
        InferenceCredentialIssuanceError::InvalidRequest(message) => {
            ApplicationError::Invalid(message)
        }
        InferenceCredentialIssuanceError::Unavailable => {
            ApplicationError::Unavailable("inference key issuance is unavailable".into())
        }
    }
}

fn identity_collision(error: &RepositoryError) -> bool {
    matches!(
        error,
        RepositoryError::Conflict(message)
            if message.contains("inference credential")
                && (message.contains("lookup prefix") || message.contains("identity"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::repositories::IInferenceCredentialRepository;
    use crate::modules::identity::infrastructure::persistence::InMemoryInferenceCredentialRepository;
    use crate::modules::projects::domain::entities::Environment;
    use crate::modules::projects::domain::repositories::IEnvironmentRepository;
    use crate::modules::projects::domain::value_objects::EnvironmentName;
    use crate::modules::secrets::domain::{
        EncryptedSecretValue, ISecretEncryptionService, SecretEncryptionError,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, IdempotentWrite, OrganizationId, ProjectId,
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
            _idempotency: IdempotencyRequest,
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

    fn command(key: &str) -> CreateInferenceKey {
        let requested_at = Utc::now();
        CreateInferenceKey {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            expires_at: requested_at + Duration::hours(1),
            idempotency_key: key.into(),
            request_id: Uuid::now_v7(),
            requested_at,
        }
    }

    fn handler() -> (
        CreateInferenceKeyHandler,
        Arc<InMemoryInferenceCredentialRepository>,
    ) {
        let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
        (
            CreateInferenceKeyHandler::new(
                Arc::new(AlwaysPresentEnvironmentRepository),
                Arc::clone(&credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>,
                InferenceCredentialIssuer::new(),
                Arc::new(TestEncryption),
            ),
            credentials,
        )
    }

    #[tokio::test]
    async fn create_returns_plaintext_once_and_stores_hash_only() {
        let (handler, _) = handler();
        let created = handler
            .execute(command("create-once"), context())
            .await
            .expect("boot")
            .expect("create");

        assert!(
            created.bearer_credential().starts_with("a3s_inf_"),
            "issued bearer must use the inference credential prefix"
        );
        assert_ne!(created.bearer_credential(), created.credential.prefix());
        let projection = created
            .credential
            .gateway_projection()
            .expect("gateway projection");
        assert!(
            projection.verifier_hash().starts_with("$argon2id$"),
            "persisted verifier must be Argon2id"
        );
        assert!(
            !projection
                .verifier_hash()
                .contains(created.bearer_credential()),
            "durable verifier must not embed the bearer secret"
        );
        assert!(
            !projection.revoked,
            "gateway ACL projection must advertise an active credential"
        );
    }

    #[tokio::test]
    async fn idempotent_replay_recovers_same_plaintext_without_reissue() {
        let (handler, _) = handler();
        let cmd = command("create-replay");
        let first = handler
            .execute(cmd.clone(), context())
            .await
            .expect("boot")
            .expect("first create");
        let replay = handler
            .execute(cmd, context())
            .await
            .expect("boot")
            .expect("replay");

        assert_eq!(
            replay.credential.id, first.credential.id,
            "idempotent replay must reuse the same credential identity"
        );
        assert_eq!(
            replay.bearer_credential(),
            first.bearer_credential(),
            "delivery receipt must recover the same one-time plaintext"
        );
        assert!(
            replay.replayed,
            "second create with the same idempotency key must be marked replayed"
        );
        assert_eq!(
            replay
                .credential
                .gateway_projection()
                .expect("projection")
                .verifier_hash(),
            first
                .credential
                .gateway_projection()
                .expect("projection")
                .verifier_hash(),
            "replay must not mint a new verifier"
        );
    }

    #[tokio::test]
    async fn list_and_debug_surfaces_never_expose_bearer_secret() {
        let (handler, credentials) = handler();
        let created = handler
            .execute(command("create-no-leak"), context())
            .await
            .expect("boot")
            .expect("create");
        let listed = credentials
            .list_inference_credentials_by_environment(
                created.credential.organization_id,
                created.credential.project_id,
                created.credential.environment_id,
            )
            .await
            .expect("list");
        let debug = format!("{listed:?}");
        let projection_debug = format!(
            "{:?}",
            listed[0].gateway_projection().expect("gateway projection")
        );

        assert_eq!(listed.len(), 1);
        assert!(
            !debug.contains(created.bearer_credential()),
            "list/debug must not leak the one-time bearer secret"
        );
        assert!(
            !projection_debug.contains(created.bearer_credential()),
            "gateway ACL projection debug must stay hash-only"
        );
    }
}
