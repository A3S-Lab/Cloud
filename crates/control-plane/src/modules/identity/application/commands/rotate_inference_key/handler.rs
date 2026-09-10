use super::RotateInferenceKey;
use crate::modules::identity::application::{
    encrypt_inference_credential_delivery_receipt, recover_inference_credential_delivery,
    IIdentityEnvironmentAccess, IdentityEnvironmentScope, InferenceCredentialDeliveryResult,
};
use crate::modules::identity::domain::events::InferenceCredentialChanged;
use crate::modules::identity::domain::repositories::{
    IInferenceCredentialLifecycleRepository, RotateInferenceCredentialWrite,
};
use crate::modules::identity::infrastructure::{
    InferenceCredentialIssuanceError, InferenceCredentialIssuer,
};
use crate::modules::secrets::domain::ISecretEncryptionService;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, RepositoryError};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use serde::Serialize;
use std::sync::Arc;

const MAX_IDENTITY_ATTEMPTS: usize = 4;

pub struct RotateInferenceKeyHandler {
    environments: Arc<dyn IIdentityEnvironmentAccess>,
    credentials: Arc<dyn IInferenceCredentialLifecycleRepository>,
    issuer: InferenceCredentialIssuer,
    encryption: Arc<dyn ISecretEncryptionService>,
}

impl RotateInferenceKeyHandler {
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

impl CommandHandler<RotateInferenceKey> for RotateInferenceKeyHandler {
    fn execute(
        &self,
        command: RotateInferenceKey,
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
            if command.expected_aggregate_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected inference credential aggregate version must be positive".into(),
                )));
            }
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
            let canonical = serde_json::to_vec(&CanonicalRotateInferenceKey {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                credential_id: command.credential_id,
                expected_aggregate_version: command.expected_aggregate_version,
                expires_at: command.expires_at,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/inference/keys/{}/rotate",
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.credential_id
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
            let credential = match credentials
                .find_inference_credential(command.organization_id, command.credential_id)
                .await
            {
                Ok(Some(value))
                    if value.project_id == command.project_id
                        && value.environment_id == command.environment_id =>
                {
                    value
                }
                Ok(_) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "inference key not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if credential.aggregate_version() != command.expected_aggregate_version {
                return Ok(Err(ApplicationError::Conflict(
                    "inference key changed before rotation".into(),
                )));
            }

            for attempt in 0..MAX_IDENTITY_ATTEMPTS {
                let (prefix, secret, verifier_hash) = match issuer.issue_rotation_material().await {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(issuance_error(error))),
                };
                let mut candidate = credential.clone();
                if let Err(error) = candidate.rotate(
                    prefix,
                    verifier_hash,
                    command.expires_at,
                    command.requested_at,
                ) {
                    return Ok(Err(ApplicationError::Invalid(error)));
                }
                let receipt = match encrypt_inference_credential_delivery_receipt(
                    encryption.as_ref(),
                    &candidate,
                    secret.as_str(),
                )
                .await
                {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error)),
                };
                let event = InferenceCredentialChanged::rotated(&candidate, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
                match credentials
                    .rotate_inference_credential(RotateInferenceCredentialWrite {
                        credential: candidate,
                        receipt,
                        expected_aggregate_version: command.expected_aggregate_version,
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
                        if identity_collision(&error) && attempt + 1 < MAX_IDENTITY_ATTEMPTS =>
                    {
                        // Prefix collision: retry with fresh material against the same head.
                    }
                    Err(error) if identity_collision(&error) => {
                        return Ok(Err(ApplicationError::Unavailable(
                            "inference key rotation exhausted its bounded identity retries".into(),
                        )))
                    }
                    Err(error) => return Ok(Err(error.into())),
                }
            }
            Ok(Err(ApplicationError::Unavailable(
                "inference key rotation is unavailable".into(),
            )))
        })
    }
}

#[derive(Serialize)]
struct CanonicalRotateInferenceKey {
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    project_id: crate::modules::shared_kernel::domain::ProjectId,
    environment_id: crate::modules::shared_kernel::domain::EnvironmentId,
    credential_id: crate::modules::shared_kernel::domain::InferenceCredentialId,
    expected_aggregate_version: u64,
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
    use crate::modules::identity::application::commands::create_inference_key::{
        CreateInferenceKey, CreateInferenceKeyHandler,
    };
    use crate::modules::identity::domain::repositories::IInferenceCredentialRepository;
    use crate::modules::identity::infrastructure::persistence::InMemoryInferenceCredentialRepository;
    use crate::modules::secrets::domain::{
        EncryptedSecretValue, ISecretEncryptionService, SecretEncryptionError,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, OrganizationId, ProjectId, RepositoryError,
    };
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

    async fn create_active_key(
        credentials: &Arc<InMemoryInferenceCredentialRepository>,
    ) -> crate::modules::identity::domain::entities::InferenceCredential {
        let requested_at = Utc::now();
        CreateInferenceKeyHandler::new(
            Arc::new(AlwaysPresentEnvironmentAccess),
            Arc::clone(credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>,
            InferenceCredentialIssuer::new(),
            Arc::new(TestEncryption),
        )
        .execute(
            CreateInferenceKey {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
                expires_at: requested_at + Duration::hours(1),
                idempotency_key: "create-for-rotate".into(),
                request_id: Uuid::now_v7(),
                requested_at,
            },
            context(),
        )
        .await
        .expect("boot")
        .expect("create")
        .credential
    }

    fn rotate_handler(
        credentials: &Arc<InMemoryInferenceCredentialRepository>,
    ) -> RotateInferenceKeyHandler {
        RotateInferenceKeyHandler::new(
            Arc::new(AlwaysPresentEnvironmentAccess),
            Arc::clone(credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>,
            InferenceCredentialIssuer::new(),
            Arc::new(TestEncryption),
        )
    }

    fn rotate_command(
        credential: &crate::modules::identity::domain::entities::InferenceCredential,
        expected_aggregate_version: u64,
        key: &str,
    ) -> RotateInferenceKey {
        let requested_at = Utc::now();
        RotateInferenceKey {
            organization_id: credential.organization_id,
            project_id: credential.project_id,
            environment_id: credential.environment_id,
            credential_id: credential.id,
            expected_aggregate_version,
            expires_at: requested_at + Duration::hours(2),
            idempotency_key: key.into(),
            request_id: Uuid::now_v7(),
            requested_at,
        }
    }

    #[tokio::test]
    async fn rotate_returns_new_plaintext_and_advances_generation() {
        let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
        let created = create_active_key(&credentials).await;
        let prior_prefix = created.prefix().to_owned();
        let prior_verifier = created
            .gateway_projection()
            .expect("projection")
            .verifier_hash()
            .to_owned();

        let rotated = rotate_handler(&credentials)
            .execute(
                rotate_command(&created, created.aggregate_version(), "rotate-once"),
                context(),
            )
            .await
            .expect("boot")
            .expect("rotate");

        assert_eq!(rotated.credential.id, created.id);
        assert_eq!(rotated.credential.generation(), 2);
        assert_eq!(
            rotated.credential.aggregate_version(),
            created.aggregate_version() + 1
        );
        assert_ne!(rotated.credential.prefix(), prior_prefix);
        assert_ne!(
            rotated
                .credential
                .gateway_projection()
                .expect("projection")
                .verifier_hash(),
            prior_verifier
        );
        assert!(rotated
            .bearer_credential()
            .starts_with(rotated.credential.prefix()));
        assert!(!rotated.bearer_credential().starts_with(&prior_prefix));
        assert!(
            !rotated
                .credential
                .gateway_projection()
                .expect("projection")
                .revoked
        );
    }

    #[tokio::test]
    async fn rotate_idempotent_replay_recovers_same_plaintext() {
        let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
        let created = create_active_key(&credentials).await;
        let cmd = rotate_command(&created, created.aggregate_version(), "rotate-replay");
        let first = rotate_handler(&credentials)
            .execute(cmd.clone(), context())
            .await
            .expect("boot")
            .expect("first rotate");
        let replay = rotate_handler(&credentials)
            .execute(cmd, context())
            .await
            .expect("boot")
            .expect("replay");

        assert!(replay.replayed);
        assert_eq!(replay.credential.id, first.credential.id);
        assert_eq!(
            replay.credential.generation(),
            first.credential.generation()
        );
        assert_eq!(replay.bearer_credential(), first.bearer_credential());
    }

    #[tokio::test]
    async fn rotate_rejects_stale_and_zero_aggregate_version() {
        let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
        let created = create_active_key(&credentials).await;
        let zero = rotate_handler(&credentials)
            .execute(rotate_command(&created, 0, "rotate-zero"), context())
            .await
            .expect("boot")
            .expect_err("zero");
        assert!(matches!(zero, ApplicationError::Invalid(_)));

        let stale = rotate_handler(&credentials)
            .execute(
                rotate_command(&created, created.aggregate_version() + 1, "rotate-stale"),
                context(),
            )
            .await
            .expect("boot")
            .expect_err("stale");
        assert!(matches!(stale, ApplicationError::Conflict(_)));
    }

    #[tokio::test]
    async fn rotate_rejects_wrong_environment_path_as_not_found() {
        let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
        let created = create_active_key(&credentials).await;
        let mut cmd = rotate_command(&created, created.aggregate_version(), "rotate-wrong-env");
        cmd.environment_id = EnvironmentId::new();
        let denied = rotate_handler(&credentials)
            .execute(cmd, context())
            .await
            .expect("boot")
            .expect_err("wrong env");
        assert!(matches!(denied, ApplicationError::NotFound(_)));
        let listed = credentials
            .list_inference_credentials_by_environment(
                created.organization_id,
                created.project_id,
                created.environment_id,
            )
            .await
            .expect("list");
        assert_eq!(listed[0].aggregate_version(), created.aggregate_version());
        assert_eq!(listed[0].generation(), created.generation());
    }

    #[tokio::test]
    async fn rotate_rejects_missing_environment_as_not_found() {
        let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
        let created = create_active_key(&credentials).await;
        let error = RotateInferenceKeyHandler::new(
            Arc::new(MissingEnvironmentAccess),
            Arc::clone(&credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>,
            InferenceCredentialIssuer::new(),
            Arc::new(TestEncryption),
        )
        .execute(
            rotate_command(&created, created.aggregate_version(), "rotate-missing-env"),
            context(),
        )
        .await
        .expect("boot")
        .expect_err("missing env");
        assert!(matches!(error, ApplicationError::NotFound(_)));
    }

    #[tokio::test]
    async fn rotate_projection_succeeds_edge_managed_snapshot_acl_succession() {
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
        let created = create_active_key(&credentials).await;
        let prior_prefix = created
            .gateway_projection()
            .expect("projection")
            .prefix
            .clone();

        let adapter = InferenceCredentialAclProjectionAdapter::new(credentials.clone());
        let scope = InferenceCredentialEnvironmentScope::new(
            created.organization_id,
            created.project_id,
            created.environment_id,
        )
        .unwrap();
        let baseline_projections = adapter
            .list_inference_credential_acl_projections(&[scope.clone()])
            .await
            .unwrap();
        assert_eq!(baseline_projections.len(), 1);
        assert_eq!(baseline_projections[0].prefix, prior_prefix);
        assert_eq!(baseline_projections[0].generation, 1);
        assert!(!baseline_projections[0].revoked);

        let node_id = NodeId::new();
        let certificate_id = GatewayCertificateId::new();
        let issued_at = Utc::now();
        let expires_at = issued_at + Duration::minutes(10);
        let now = issued_at;
        let workload_id = WorkloadId::new();
        let workload_revision_id = WorkloadRevisionId::new();
        let mut owned = Route::create(
            RouteId::new(),
            created.organization_id,
            created.project_id,
            created.environment_id,
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
                &baseline_projections,
            )
            .unwrap();
        assert!(baseline
            .acl
            .contains(&format!("prefix = \"{prior_prefix}\"")));
        assert!(baseline.acl.contains("generation = 1"));
        assert!(!baseline.acl.contains("\n  workers "));

        let rotated = rotate_handler(&credentials)
            .execute(
                rotate_command(&created, created.aggregate_version(), "rotate-for-edge"),
                context(),
            )
            .await
            .expect("boot")
            .expect("rotate");
        let next_prefix = rotated
            .credential
            .gateway_projection()
            .expect("projection")
            .prefix
            .clone();

        let successor_projections = adapter
            .list_inference_credential_acl_projections(&[scope])
            .await
            .unwrap();
        assert_eq!(successor_projections.len(), 1);
        assert_eq!(successor_projections[0].credential_id, created.id.as_uuid());
        assert_eq!(successor_projections[0].generation, 2);
        assert_eq!(successor_projections[0].prefix, next_prefix);
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
        assert!(successor
            .acl
            .contains(&format!("prefix = \"{next_prefix}\"")));
        assert!(!successor
            .acl
            .contains(&format!("prefix = \"{prior_prefix}\"")));
        assert!(successor
            .acl
            .contains(&format!("credentials \"{}\"", created.id.as_uuid())));
        assert!(successor.acl.contains("generation = 2"));
        assert!(successor.acl.contains("revoked = false"));
        assert!(!successor.acl.contains("\n  workers "));
        assert_eq!(successor.acl.matches("inference {").count(), 1);
        assert!(
            !successor.acl.contains(rotated.bearer_credential()),
            "managed-snapshot ACL must never embed the rotated bearer secret"
        );
        assert_eq!(successor_projections[0].generation, 2);
        assert_ne!(successor_projections[0].prefix, prior_prefix);
    }
}
