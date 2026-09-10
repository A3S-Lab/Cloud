use super::CreateInferenceKey;
use crate::modules::identity::application::{
    encrypt_inference_credential_delivery_receipt, recover_inference_credential_delivery,
    IIdentityEnvironmentAccess, IdentityEnvironmentScope, InferenceCredentialDeliveryResult,
};
use crate::modules::identity::domain::events::InferenceCredentialChanged;
use crate::modules::identity::domain::repositories::{
    CreateInferenceCredentialWrite, IInferenceCredentialLifecycleRepository,
};
use crate::modules::identity::infrastructure::{
    InferenceCredentialIssuanceError, InferenceCredentialIssueRequest, InferenceCredentialIssuer,
};
use crate::modules::secrets::domain::ISecretEncryptionService;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, RepositoryError};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use serde::Serialize;
use std::sync::Arc;

const MAX_IDENTITY_ATTEMPTS: usize = 4;

pub struct CreateInferenceKeyHandler {
    environments: Arc<dyn IIdentityEnvironmentAccess>,
    credentials: Arc<dyn IInferenceCredentialLifecycleRepository>,
    issuer: InferenceCredentialIssuer,
    encryption: Arc<dyn ISecretEncryptionService>,
}

impl CreateInferenceKeyHandler {
    pub fn new(
        environments: Arc<dyn IIdentityEnvironmentAccess>,
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
            let environment_scope = match IdentityEnvironmentScope::new(
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
    use crate::modules::secrets::domain::{
        EncryptedSecretValue, ISecretEncryptionService, SecretEncryptionError,
    };
    use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
    use a3s_boot::{CommandHandler, ModuleRef};
    use async_trait::async_trait;
    use base64::engine::general_purpose::STANDARD_NO_PAD;
    use base64::Engine as _;
    use chrono::{Duration, Utc};
    use sha2::{Digest, Sha256};
    use std::sync::Arc;
    use uuid::Uuid;

    struct AlwaysPresentEnvironmentAccess;

    #[async_trait]
    impl IIdentityEnvironmentAccess for AlwaysPresentEnvironmentAccess {
        async fn environment_exists(
            &self,
            _scope: IdentityEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    struct MissingEnvironmentAccess;

    #[async_trait]
    impl IIdentityEnvironmentAccess for MissingEnvironmentAccess {
        async fn environment_exists(
            &self,
            _scope: IdentityEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            Ok(false)
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
                Arc::new(AlwaysPresentEnvironmentAccess),
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

    #[tokio::test]
    async fn missing_environment_fails_closed_as_not_found() {
        let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
        let handler = CreateInferenceKeyHandler::new(
            Arc::new(MissingEnvironmentAccess),
            Arc::clone(&credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>,
            InferenceCredentialIssuer::new(),
            Arc::new(TestEncryption),
        );
        let cmd = command("missing-env");
        let error = handler
            .execute(cmd.clone(), context())
            .await
            .expect("boot")
            .expect_err("missing environment");
        assert!(matches!(error, ApplicationError::NotFound(_)));
        assert!(
            credentials
                .list_inference_credentials_by_environment(
                    cmd.organization_id,
                    cmd.project_id,
                    cmd.environment_id,
                )
                .await
                .unwrap()
                .is_empty(),
            "missing environment must not leave a credential row"
        );
    }

    #[tokio::test]
    async fn idempotent_replay_after_receipt_sweep_fails_closed_without_reissue() {
        use crate::modules::identity::application::InferenceCredentialDeliveryReceiptSweeper;
        use crate::modules::shared_kernel::application::ApplicationError;
        use std::time::Duration as StdDuration;

        let (handler, credentials) = handler();
        let cmd = command("create-then-sweep");
        let first = handler
            .execute(cmd.clone(), context())
            .await
            .expect("boot")
            .expect("first create");
        let bearer = first.bearer_credential().to_owned();
        let delivery_expires_at = first.delivery_expires_at;

        let sweeper = InferenceCredentialDeliveryReceiptSweeper::new(
            Arc::clone(&credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>,
            StdDuration::from_secs(60),
            100,
        )
        .expect("bounded sweeper");
        assert_eq!(
            sweeper.run_once(delivery_expires_at).await.expect("sweep"),
            1,
            "expired delivery receipt must be swept exactly once"
        );

        let error = handler
            .execute(
                CreateInferenceKey {
                    requested_at: delivery_expires_at + Duration::seconds(1),
                    ..cmd
                },
                context(),
            )
            .await
            .expect("boot")
            .expect_err("swept receipt must not recover plaintext");
        match error {
            ApplicationError::Conflict(message) => assert!(
                message.contains("no longer recoverable") || message.contains("expired"),
                "expected delivery fail-closed conflict, got {message}"
            ),
            other => panic!("expected Conflict after receipt sweep, got {other:?}"),
        }

        let listed = credentials
            .list_inference_credentials_by_environment(
                first.credential.organization_id,
                first.credential.project_id,
                first.credential.environment_id,
            )
            .await
            .expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, first.credential.id);
        assert_eq!(
            listed[0]
                .gateway_projection()
                .expect("projection")
                .verifier_hash(),
            first
                .credential
                .gateway_projection()
                .expect("projection")
                .verifier_hash(),
            "sweep must not reissue or mutate the durable credential"
        );
        let debug = format!("{listed:?}");
        assert!(
            !debug.contains(&bearer),
            "post-sweep state must not regenerate or leak the swept bearer"
        );
    }

    #[tokio::test]
    async fn create_projection_succeeds_edge_managed_snapshot_acl_succession() {
        use crate::modules::edge::domain::{
            DomainNamePattern, Route, RouteHostname, RoutePath, RoutePortName, RouteState,
            RouteTarget, UpstreamEndpoint,
        };
        use crate::modules::edge::infrastructure::{
            GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, GatewaySnapshotMetadata,
        };
        use crate::modules::identity::application::{
            IInferenceCredentialAclProjectionPort, InferenceCredentialEnvironmentScope,
        };
        use crate::modules::identity::infrastructure::InferenceCredentialAclProjectionAdapter;
        use crate::modules::shared_kernel::domain::{
            DomainClaimId, GatewayCertificateId, GatewayScopeId, NodeId, RouteId, WorkloadId,
            WorkloadRevisionId,
        };

        let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let adapter = InferenceCredentialAclProjectionAdapter::new(credentials.clone());
        let scope =
            InferenceCredentialEnvironmentScope::new(organization_id, project_id, environment_id)
                .unwrap();

        let empty_projections = adapter
            .list_inference_credential_acl_projections(&[scope.clone()])
            .await
            .unwrap();
        assert!(empty_projections.is_empty());

        let node_id = NodeId::new();
        let certificate_id = GatewayCertificateId::new();
        let issued_at = Utc::now();
        let expires_at = issued_at + Duration::minutes(10);
        let now = issued_at;
        let workload_id = WorkloadId::new();
        let workload_revision_id = WorkloadRevisionId::new();
        let mut owned = Route::create(
            RouteId::new(),
            organization_id,
            project_id,
            environment_id,
            GatewayScopeId::new(),
            node_id,
            RouteHostname::parse("api.example.com").unwrap(),
            RoutePath::parse("/v1").unwrap(),
            DomainClaimId::new(),
            DomainNamePattern::parse("api.example.com").unwrap(),
            certificate_id,
            workload_id,
            RouteTarget::new(
                workload_id,
                workload_revision_id,
                format!("workload:{workload_id}:revision:{workload_revision_id}"),
                1,
                RoutePortName::parse("http").unwrap(),
                UpstreamEndpoint::parse("http://127.0.0.1:49152").unwrap(),
                now,
            )
            .unwrap(),
            now,
        )
        .unwrap();
        owned.state = RouteState::Active;
        owned.gateway_certificate_id = Some(certificate_id);

        let compiler = GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
            entrypoint_address: "0.0.0.0:8081".into(),
            management_address: "127.0.0.1:9090".into(),
            management_path_prefix: "/api/gateway".into(),
            management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
            upstream_request_timeout_ms: 30_000,
            certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
            managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
        })
        .unwrap();

        let baseline = compiler
            .compile_certificate_convergence_with_inference_credentials(
                GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
                Some(certificate_id),
                &[owned.clone()],
                &empty_projections,
            )
            .unwrap();
        assert!(!baseline.acl.contains("prefix = \"a3s_inf_"));
        assert!(!baseline.acl.contains("revoked = false"));
        assert!(!baseline.acl.contains("\n  workers "));

        let requested_at = Utc::now();
        let created = CreateInferenceKeyHandler::new(
            Arc::new(AlwaysPresentEnvironmentAccess),
            Arc::clone(&credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>,
            InferenceCredentialIssuer::new(),
            Arc::new(TestEncryption),
        )
        .execute(
            CreateInferenceKey {
                organization_id,
                project_id,
                environment_id,
                expires_at: requested_at + Duration::hours(1),
                idempotency_key: "create-for-edge-empty-baseline".into(),
                request_id: Uuid::now_v7(),
                requested_at,
            },
            context(),
        )
        .await
        .expect("boot")
        .expect("create");
        let prefix = created
            .credential
            .gateway_projection()
            .expect("active projection")
            .prefix
            .clone();

        let successor_projections = adapter
            .list_inference_credential_acl_projections(&[scope])
            .await
            .unwrap();
        assert_eq!(successor_projections.len(), 1);
        assert_eq!(
            successor_projections[0].credential_id,
            created.credential.id.as_uuid()
        );
        assert_eq!(successor_projections[0].prefix, prefix);
        assert!(!successor_projections[0].revoked);

        let successor = compiler
            .compile_certificate_convergence_with_inference_credentials(
                GatewaySnapshotMetadata::new(
                    node_id,
                    3,
                    Some(2),
                    issued_at + Duration::seconds(1),
                    expires_at + Duration::seconds(1),
                ),
                Some(certificate_id),
                &[owned],
                &successor_projections,
            )
            .unwrap();
        assert!(successor.acl.contains(&format!("prefix = \"{prefix}\"")));
        assert!(successor.acl.contains("revoked = false"));
        assert!(!successor.acl.contains("revoked = true"));
        assert!(!successor.acl.contains("\n  workers "));
        assert_eq!(successor.acl.matches("inference {").count(), 1);
        assert!(
            !successor.acl.contains(created.bearer_credential()),
            "managed-snapshot ACL must never embed the one-time bearer secret"
        );
    }
}
