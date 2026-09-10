use super::RevokeInferenceKey;
use crate::modules::identity::application::InferenceCredentialMutationResult;
use crate::modules::identity::domain::events::InferenceCredentialChanged;
use crate::modules::identity::domain::repositories::{
    IInferenceCredentialLifecycleRepository, RevokeInferenceCredentialWrite,
};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use serde::Serialize;
use std::sync::Arc;

pub struct RevokeInferenceKeyHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    credentials: Arc<dyn IInferenceCredentialLifecycleRepository>,
}

impl RevokeInferenceKeyHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        credentials: Arc<dyn IInferenceCredentialLifecycleRepository>,
    ) -> Self {
        Self {
            environments,
            credentials,
        }
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
        let environments = Arc::clone(&self.environments);
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            if command.expected_aggregate_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected inference credential aggregate version must be positive".into(),
                )));
            }
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
            let canonical = serde_json::to_vec(&CanonicalRevokeInferenceKey {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                credential_id: command.credential_id,
                expected_aggregate_version: command.expected_aggregate_version,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/inference/keys/{}/revoke",
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
    project_id: crate::modules::shared_kernel::domain::ProjectId,
    environment_id: crate::modules::shared_kernel::domain::EnvironmentId,
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
        EnvironmentId, IdempotentWrite, InferenceCredentialId, OrganizationId, ProjectId,
        RepositoryError,
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

    struct MissingEnvironmentRepository;

    #[async_trait]
    impl IEnvironmentRepository for MissingEnvironmentRepository {
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
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
        ) -> Result<Option<Environment>, RepositoryError> {
            Ok(None)
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
        ) -> Result<Vec<Environment>, RepositoryError> {
            Ok(Vec::new())
        }
    }

    fn revoke_handler(
        credentials: &Arc<InMemoryInferenceCredentialRepository>,
    ) -> RevokeInferenceKeyHandler {
        RevokeInferenceKeyHandler::new(
            Arc::new(AlwaysPresentEnvironmentRepository),
            Arc::clone(credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>,
        )
    }

    fn revoke_command(
        credential: &crate::modules::identity::domain::entities::InferenceCredential,
        expected_aggregate_version: u64,
        idempotency_key: &str,
    ) -> RevokeInferenceKey {
        RevokeInferenceKey {
            organization_id: credential.organization_id,
            project_id: credential.project_id,
            environment_id: credential.environment_id,
            credential_id: credential.id,
            expected_aggregate_version,
            idempotency_key: idempotency_key.into(),
            request_id: Uuid::now_v7(),
            requested_at: credential.updated_at() + Duration::seconds(1),
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
        let revoked = revoke_handler(&credentials)
            .execute(
                revoke_command(&credential, credential.aggregate_version(), "revoke-once"),
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
        let command = revoke_command(&credential, credential.aggregate_version(), "revoke-replay");
        let handler = revoke_handler(&credentials);
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

    #[tokio::test]
    async fn missing_key_fails_closed_as_not_found() {
        let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
        let error = revoke_handler(&credentials)
            .execute(
                RevokeInferenceKey {
                    organization_id: OrganizationId::new(),
                    project_id: ProjectId::new(),
                    environment_id: EnvironmentId::new(),
                    credential_id: InferenceCredentialId::new(),
                    expected_aggregate_version: 1,
                    idempotency_key: "revoke-missing".into(),
                    request_id: Uuid::now_v7(),
                    requested_at: Utc::now(),
                },
                context(),
            )
            .await
            .expect("boot")
            .expect_err("missing key");
        assert!(matches!(error, ApplicationError::NotFound(_)));
    }

    #[tokio::test]
    async fn stale_aggregate_version_fails_closed_as_conflict() {
        let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
        let credential = create_active_key(&credentials).await;
        let error = revoke_handler(&credentials)
            .execute(
                revoke_command(
                    &credential,
                    credential.aggregate_version() + 1,
                    "revoke-stale",
                ),
                context(),
            )
            .await
            .expect("boot")
            .expect_err("stale version");
        assert!(matches!(error, ApplicationError::Conflict(_)));
        let listed = credentials
            .list_inference_credentials_by_environment(
                credential.organization_id,
                credential.project_id,
                credential.environment_id,
            )
            .await
            .expect("list");
        assert!(
            listed[0].revoked_at().is_none(),
            "stale revoke must not mutate the active credential"
        );
    }

    #[tokio::test]
    async fn revoke_rejects_wrong_environment_path_as_not_found() {
        let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
        let credential = create_active_key(&credentials).await;
        let error = revoke_handler(&credentials)
            .execute(
                RevokeInferenceKey {
                    organization_id: credential.organization_id,
                    project_id: credential.project_id,
                    environment_id: EnvironmentId::new(),
                    credential_id: credential.id,
                    expected_aggregate_version: credential.aggregate_version(),
                    idempotency_key: "revoke-wrong-env".into(),
                    request_id: Uuid::now_v7(),
                    requested_at: credential.updated_at() + Duration::seconds(1),
                },
                context(),
            )
            .await
            .expect("boot")
            .expect_err("wrong environment path");
        assert!(matches!(error, ApplicationError::NotFound(_)));
        let listed = credentials
            .list_inference_credentials_by_environment(
                credential.organization_id,
                credential.project_id,
                credential.environment_id,
            )
            .await
            .expect("list");
        assert!(
            listed[0].revoked_at().is_none(),
            "wrong-environment revoke must not mutate the active credential"
        );
    }

    #[tokio::test]
    async fn revoke_rejects_missing_environment_as_not_found() {
        let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
        let credential = create_active_key(&credentials).await;
        let error = RevokeInferenceKeyHandler::new(
            Arc::new(MissingEnvironmentRepository),
            Arc::clone(&credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>,
        )
        .execute(
            revoke_command(
                &credential,
                credential.aggregate_version(),
                "revoke-missing-env",
            ),
            context(),
        )
        .await
        .expect("boot")
        .expect_err("missing environment");
        assert!(matches!(error, ApplicationError::NotFound(_)));
        let listed = credentials
            .list_inference_credentials_by_environment(
                credential.organization_id,
                credential.project_id,
                credential.environment_id,
            )
            .await
            .expect("list");
        assert!(
            listed[0].revoked_at().is_none(),
            "missing-environment revoke must not mutate the active credential"
        );
    }

    #[tokio::test]
    async fn revoke_projection_succeeds_edge_managed_snapshot_acl_succession() {
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
        let requested_at = Utc::now();
        let created = CreateInferenceKeyHandler::new(
            Arc::new(AlwaysPresentEnvironmentRepository),
            Arc::clone(&credentials)
                as Arc<dyn crate::modules::identity::domain::repositories::IInferenceCredentialLifecycleRepository>,
            InferenceCredentialIssuer::new(),
            Arc::new(TestEncryption),
        )
        .execute(
            CreateInferenceKey {
                organization_id,
                project_id,
                environment_id,
                expires_at: requested_at + Duration::hours(1),
                idempotency_key: "create-for-edge-succession".into(),
                request_id: Uuid::now_v7(),
                requested_at,
            },
            context(),
        )
        .await
        .expect("boot")
        .expect("create");
        let credential = created.credential;
        let prefix = credential
            .gateway_projection()
            .expect("active projection")
            .prefix
            .clone();

        let adapter = InferenceCredentialAclProjectionAdapter::new(credentials.clone());
        let scope =
            InferenceCredentialEnvironmentScope::new(organization_id, project_id, environment_id)
                .unwrap();
        let baseline_projections = adapter
            .list_inference_credential_acl_projections(&[scope.clone()])
            .await
            .unwrap();
        assert_eq!(baseline_projections.len(), 1);
        assert!(!baseline_projections[0].revoked);
        assert_eq!(baseline_projections[0].credential_id, credential.id.as_uuid());
        assert_eq!(baseline_projections[0].prefix, prefix);

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
                &baseline_projections,
            )
            .unwrap();
        assert!(baseline.acl.contains(&format!("prefix = \"{prefix}\"")));
        assert!(baseline.acl.contains("revoked = false"));
        assert!(!baseline.acl.contains("revoked = true"));
        assert!(!baseline.acl.contains("\n  workers "));

        let revoked = revoke_handler(&credentials)
            .execute(
                revoke_command(
                    &credential,
                    credential.aggregate_version(),
                    "revoke-for-edge-succession",
                ),
                context(),
            )
            .await
            .expect("boot")
            .expect("revoke");
        assert!(revoked
            .credential
            .gateway_projection()
            .expect("projection")
            .revoked);

        let successor_projections = adapter
            .list_inference_credential_acl_projections(&[scope])
            .await
            .unwrap();
        assert_eq!(successor_projections.len(), 1);
        assert_eq!(
            successor_projections[0].credential_id,
            credential.id.as_uuid()
        );
        assert_eq!(
            successor_projections[0].generation,
            baseline_projections[0].generation
        );
        assert_eq!(successor_projections[0].prefix, prefix);
        assert!(successor_projections[0].revoked);

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
        assert!(successor.acl.contains("revoked = true"));
        assert!(!successor.acl.contains("revoked = false"));
        assert!(!successor.acl.contains("\n  workers "));
        assert_eq!(successor.acl.matches("inference {").count(), 1);
    }
}
