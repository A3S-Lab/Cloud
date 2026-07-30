use super::context;
use crate::modules::edge::{
    GetMcpCredential, GetMcpCredentialHandler, IssueMcpCredential, IssueMcpCredentialHandler,
    ListMcpCredentials, ListMcpCredentialsHandler, McpCredentialLifecycleService,
    RevokeMcpCredential, RevokeMcpCredentialHandler, RotateMcpCredential,
    RotateMcpCredentialHandler,
};
use crate::modules::edge::{IMcpCredentialRepository, InMemoryEdgeRepository};
use crate::modules::fleet::LocalKeyEncryptionService;
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::secrets::domain::{
    EncryptedSecretValue, ISecretEncryptionService, SecretEncryptionError,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
};
use a3s_boot::{CommandHandler, QueryHandler};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{Duration, TimeZone, Utc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::TempDir;
use uuid::Uuid;

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0)
        .single()
        .expect("time")
}

struct ExactEnvironment(Environment);

#[async_trait]
impl IEnvironmentRepository for ExactEnvironment {
    async fn create(
        &self,
        _environment: Environment,
        _event: DomainEventEnvelope,
        _idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
        Err(RepositoryError::Storage(
            "test environment is immutable".into(),
        ))
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Option<Environment>, RepositoryError> {
        Ok((self.0.organization_id == organization_id
            && self.0.project_id == project_id
            && self.0.id == environment_id)
            .then(|| self.0.clone()))
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<Environment>, RepositoryError> {
        Ok(
            (self.0.organization_id == organization_id && self.0.project_id == project_id)
                .then(|| self.0.clone())
                .into_iter()
                .collect(),
        )
    }
}

struct FailingDecryptEncryption {
    inner: Arc<dyn ISecretEncryptionService>,
    remaining_failures: AtomicUsize,
}

impl FailingDecryptEncryption {
    fn new(inner: Arc<dyn ISecretEncryptionService>, failures: usize) -> Self {
        Self {
            inner,
            remaining_failures: AtomicUsize::new(failures),
        }
    }
}

#[async_trait]
impl ISecretEncryptionService for FailingDecryptEncryption {
    async fn encrypt(
        &self,
        plaintext: &[u8],
        context: &[u8],
    ) -> Result<EncryptedSecretValue, SecretEncryptionError> {
        self.inner.encrypt(plaintext, context).await
    }

    async fn decrypt(
        &self,
        value: &EncryptedSecretValue,
        context: &[u8],
    ) -> Result<Vec<u8>, SecretEncryptionError> {
        if self
            .remaining_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(SecretEncryptionError::Unavailable(
                "injected failure containing provider detail".into(),
            ));
        }
        self.inner.decrypt(value, context).await
    }

    async fn health(&self) -> Result<bool, SecretEncryptionError> {
        self.inner.health().await
    }
}

struct Fixture {
    _directory: TempDir,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    repository: Arc<InMemoryEdgeRepository>,
    lifecycle: McpCredentialLifecycleService,
    environments: Arc<dyn IEnvironmentRepository>,
}

fn fixture(decrypt_failures: usize) -> Fixture {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let directory = TempDir::new().expect("temporary key directory");
    let encryption: Arc<dyn ISecretEncryptionService> = Arc::new(
        LocalKeyEncryptionService::load_or_create(directory.path().join("delivery.key"))
            .expect("local encryption"),
    );
    let encryption: Arc<dyn ISecretEncryptionService> =
        Arc::new(FailingDecryptEncryption::new(encryption, decrypt_failures));
    let repository = Arc::new(InMemoryEdgeRepository::new());
    let lifecycle =
        McpCredentialLifecycleService::new(repository.clone(), encryption, Duration::minutes(10))
            .expect("lifecycle service");
    let environments: Arc<dyn IEnvironmentRepository> =
        Arc::new(ExactEnvironment(Environment::create(
            organization_id,
            project_id,
            environment_id,
            EnvironmentName::parse("production").expect("environment name"),
            now(),
        )));
    Fixture {
        _directory: directory,
        organization_id,
        project_id,
        environment_id,
        repository,
        lifecycle,
        environments,
    }
}

fn issue(fixture: &Fixture, key: &str) -> IssueMcpCredential {
    IssueMcpCredential {
        organization_id: fixture.organization_id,
        project_id: fixture.project_id,
        environment_id: fixture.environment_id,
        expires_at: now() + Duration::days(30),
        idempotency_key: key.into(),
        request_id: Uuid::new_v4(),
        requested_at: now(),
    }
}

#[tokio::test]
async fn recovers_the_exact_committed_secret_after_first_response_decryption_fails() {
    let fixture = fixture(1);
    let handler = IssueMcpCredentialHandler::new(
        Arc::clone(&fixture.environments),
        fixture.lifecycle.clone(),
    );
    let first = handler
        .execute(issue(&fixture, "recover-issue"), context())
        .await
        .expect("command bus")
        .expect_err("injected post-commit failure");
    assert_eq!(
        first,
        ApplicationError::Unavailable(
            "MCP credential encrypted delivery is temporarily unavailable".into()
        )
    );
    let mut retry = issue(&fixture, "recover-issue");
    retry.request_id = Uuid::new_v4();
    retry.requested_at += Duration::seconds(1);
    let recovered = handler
        .execute(retry, context())
        .await
        .expect("command bus")
        .expect("recover committed issuance");
    assert!(recovered.replayed);
    assert_eq!(recovered.credential.generation(), 1);
    assert_eq!(
        recovered.secret().expect("one-time secret").expose().len(),
        88
    );
    assert_eq!(
        fixture
            .repository
            .list_mcp_credentials(
                fixture.organization_id,
                fixture.project_id,
                fixture.environment_id,
            )
            .await
            .expect("list persisted credentials")
            .len(),
        1
    );
    assert_eq!(fixture.repository.outbox_events().await.len(), 1);
}

#[tokio::test]
async fn issues_rotates_revokes_and_queries_one_exact_tenant_credential() {
    let fixture = fixture(0);
    let issue_handler = IssueMcpCredentialHandler::new(
        Arc::clone(&fixture.environments),
        fixture.lifecycle.clone(),
    );
    let issued = issue_handler
        .execute(issue(&fixture, "issue-key"), context())
        .await
        .expect("command bus")
        .expect("issue credential");
    let issued_secret = issued.secret().expect("issued secret").expose().to_owned();
    assert!(!issued.replayed);
    assert!(!format!("{issued:?}").contains(&issued_secret));
    let mut conflicting_issue = issue(&fixture, "issue-key");
    conflicting_issue.expires_at += Duration::days(1);
    assert!(matches!(
        issue_handler
            .execute(conflicting_issue, context())
            .await
            .expect("command bus"),
        Err(ApplicationError::Conflict(message))
            if message == "idempotency key reused with different input"
    ));
    assert_eq!(fixture.repository.outbox_events().await.len(), 1);

    let rotate_handler = RotateMcpCredentialHandler::new(fixture.lifecycle.clone());
    let rotate = RotateMcpCredential {
        organization_id: fixture.organization_id,
        project_id: fixture.project_id,
        environment_id: fixture.environment_id,
        credential_id: issued.credential.id,
        expires_at: now() + Duration::days(60),
        idempotency_key: "rotate-key".into(),
        request_id: Uuid::new_v4(),
        requested_at: now() + Duration::minutes(1),
    };
    let rotated = rotate_handler
        .execute(rotate.clone(), context())
        .await
        .expect("command bus")
        .expect("rotate credential");
    let rotated_secret = rotated
        .secret()
        .expect("rotated secret")
        .expose()
        .to_owned();
    assert_ne!(rotated_secret, issued_secret);
    assert_eq!(rotated.credential.generation(), 2);
    let replayed_rotation = rotate_handler
        .execute(
            RotateMcpCredential {
                request_id: Uuid::new_v4(),
                requested_at: rotate.requested_at + Duration::seconds(1),
                ..rotate
            },
            context(),
        )
        .await
        .expect("command bus")
        .expect("replay rotation");
    assert!(replayed_rotation.replayed);
    assert_eq!(
        replayed_rotation
            .secret()
            .expect("same rotated secret")
            .expose(),
        rotated_secret
    );
    assert!(matches!(
        issue_handler
            .execute(issue(&fixture, "issue-key"), context())
            .await
            .expect("command bus"),
        Err(ApplicationError::Conflict(_))
    ));

    let get_handler = GetMcpCredentialHandler::new(fixture.repository.clone());
    assert_eq!(
        get_handler
            .execute(
                GetMcpCredential {
                    organization_id: fixture.organization_id,
                    project_id: fixture.project_id,
                    environment_id: fixture.environment_id,
                    credential_id: rotated.credential.id,
                },
                context(),
            )
            .await
            .expect("query bus")
            .expect("get credential"),
        rotated.credential
    );
    let list_handler = ListMcpCredentialsHandler::new(fixture.repository.clone());
    assert_eq!(
        list_handler
            .execute(
                ListMcpCredentials {
                    organization_id: fixture.organization_id,
                    project_id: fixture.project_id,
                    environment_id: fixture.environment_id,
                },
                context(),
            )
            .await
            .expect("query bus")
            .expect("list credentials")
            .len(),
        1
    );

    let revoke_handler = RevokeMcpCredentialHandler::new(fixture.lifecycle);
    let revoke = RevokeMcpCredential {
        organization_id: fixture.organization_id,
        project_id: fixture.project_id,
        environment_id: fixture.environment_id,
        credential_id: rotated.credential.id,
        idempotency_key: "revoke-key".into(),
        request_id: Uuid::new_v4(),
        requested_at: now() + Duration::minutes(2),
    };
    let revoked = revoke_handler
        .execute(revoke.clone(), context())
        .await
        .expect("command bus")
        .expect("revoke credential");
    assert!(revoked.credential.revoked_at().is_some());
    assert!(revoked.secret().is_none());
    let replayed_revoke = revoke_handler
        .execute(
            RevokeMcpCredential {
                request_id: Uuid::new_v4(),
                requested_at: revoke.requested_at + Duration::seconds(1),
                ..revoke
            },
            context(),
        )
        .await
        .expect("command bus")
        .expect("replay revocation");
    assert!(replayed_revoke.replayed);
    assert!(replayed_revoke.secret().is_none());
}
