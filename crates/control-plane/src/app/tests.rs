use super::*;
use crate::config::{
    ArtifactTransferConfig, AssetsConfig, AuditConfig, AuthConfig, BuildsConfig, DeploymentsConfig,
    EdgeConfig, EventProviderKind, EventsConfig, FleetConfig, HumanTasksConfig, LogsConfig,
    NodeControlConfig, ObjectStorageConfig, ObjectStorageProviderKind, OperationsConfig,
    PostgresConfig, ProcessRole, RegistryConfig, SecurityConfig, SecurityProfile,
    SecurityProviderKind, ServerConfig, SmtpConfig, SourcesConfig,
};
use crate::modules::agents::{BuiltInAgentExecutionProviderRegistry, InMemoryAgentRepository};
use crate::modules::artifacts::{
    BuildEvidenceSigningError, BuildEvidenceSigningKey, IBuildEvidenceSigner,
    InMemoryBuildRunRepository, VerifiedBuildEvidenceSignature,
};
use crate::modules::audit::{
    AuditAttributionStatus, AuditExportSigningError, AuditExportSigningKey, AuditRecord,
    IAuditExportSigner, InMemoryAuditRecordRepository, VerifiedAuditExportSignature,
};
use crate::modules::connectors::{
    InMemoryConnectorExecutionRepository, InMemoryConnectorProfileRepository,
};
use crate::modules::developer_workflows::{
    BuildPlanSourceLayoutError, BuildPlanSourceLayoutRequest, IBuildPlanSourceLayoutPort,
    InMemoryBuildPlanRepository, InMemoryPullRequestPreviewPolicyRepository,
    InMemoryPullRequestPreviewProjectionRepository, InMemoryWorkloadProfileRepository,
};
use crate::modules::edge::domain::repositories::{
    IMcpRoutePolicyRepository, McpRoutePolicyWrite, MutateMcpRoutePolicyWrite,
};
use crate::modules::edge::domain::McpRoutePolicy;
use crate::modules::executions::{
    InMemoryExecutionRepository, InMemoryExecutionTemplateRepository,
};
use crate::modules::files::{
    IUserFileObjectStore, InMemoryUserFileRepository, UserFileContentReference,
    UserFileObjectError, UserFileObjectReader, UserFileObjectWrite,
};
use crate::modules::fleet::domain::entities::{NodeCertificate, NodeCertificateMaterial};
use crate::modules::fleet::domain::services::{CertificateAuthorityError, NodeCertificateRequest};
use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
use crate::modules::forms::{InMemoryFormRepository, NativeFormSemanticCore};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::InMemoryIdentityRepository;
use crate::modules::operations::InMemoryOperationRepository;
use crate::modules::plugins::domain::entities::PluginRegistry;
use crate::modules::plugins::domain::services::{
    IPluginRegistryCatalog, IPluginRegistryEnrollmentAuthorizer, PluginRegistryCatalogError,
    PluginRegistryEnrollmentAuthorizationError,
};
use crate::modules::plugins::{InMemoryPluginRegistryRepository, PluginTrustRootObjectStore};
use crate::modules::projects::InMemoryProjectsRepository;
use crate::modules::search::{ISearchRepository, InMemorySearchRepository};
use crate::modules::secrets::{
    EncryptedSecretValue, ISecretEncryptionService, InMemorySecretRepository, SecretEncryptionError,
};
use crate::modules::security::InMemoryGatewayRoutePolicyTimelineRepository;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GatewayScopeId, HumanTaskId, IdempotencyRequest, IdempotentWrite,
    OrganizationId, PrincipalId, ProjectAttributionProfileId, ProjectId, RepositoryError, RouteId,
    WorkflowDecisionId,
};
use crate::modules::sources::domain::{
    GitReference, GithubAccountId, GithubAccountKind, GithubAppAuthorizationError,
    GithubInstallationTokenError, GithubInstallationTokenRequest,
    GithubInstallationVerificationRequest, GithubLogin, IGithubAppAuthorizationService,
    ISourceResolver, ResolvedSource, SourceProviderCredential, SourceResolutionError,
    SourceResolutionRequest, SourceWebhookPayload, VerifiedGithubInstallation,
};
use crate::modules::sources::{
    GithubWebhookVerifier, InMemoryGithubConnectionRepository, InMemorySourceRevisionRepository,
};
use crate::modules::workflow::{
    ChangeHumanTaskWrite, CreateHumanTaskWrite, DecideHumanTaskWrite, FlowResumeReceipt,
    HumanTaskDecisionRecord, HumanTaskRecord, HumanTaskResumeDelivery, HumanTaskStatus,
    IHumanTaskRepository, IWorkflowRunHistoryReader, IWorkflowRunVariableReader,
    InMemoryOntologyRepository, InMemoryWorkflowDefinitionRepository,
    InMemoryWorkflowGoalRepository, InMemoryWorkflowRunRepository, WorkflowDecisionOutcome,
    WorkflowRunDiagnosticsReader, WorkflowRunFlowRuntime, WorkflowRunHistoryPage,
    WorkflowRunRecord, WorkflowRunVariableInspection,
};
use crate::modules::workloads::{
    IOciArtifactResolver, InMemoryWorkloadRepository, OciArtifact, OciArtifactReference,
    OciArtifactResolutionError, OciRegistryCredentialReference,
};
use a3s_boot::{BootError, BootRequest, BootResponse, HttpMethod};
use a3s_flow::FlowEngine;
use a3s_use_core::PluginReleaseChannel;
use a3s_use_extension::{
    PluginCatalogHost, PluginCatalogInspection, PluginCatalogPage, PluginCatalogSearch,
    VerifiedRegistryMetadata, MAX_BOOTSTRAP_ROOT_BYTES,
};
use async_trait::async_trait;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use base64::Engine as _;
use chrono::{DateTime, Utc};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

struct TestAuditExportSigner {
    key: Ed25519KeyPair,
    key_id: String,
}

impl TestAuditExportSigner {
    fn new() -> Self {
        let key = Ed25519KeyPair::from_seed_unchecked(&[0x43; 32])
            .expect("test audit export Ed25519 key");
        let key_id = format!("{:x}", Sha256::digest(key.public_key().as_ref()));
        Self { key, key_id }
    }
}

#[async_trait]
impl IAuditExportSigner for TestAuditExportSigner {
    async fn sign(
        &self,
        pae: &[u8],
    ) -> std::result::Result<VerifiedAuditExportSignature, AuditExportSigningError> {
        VerifiedAuditExportSignature::new(
            AuditExportSigningKey {
                algorithm: "ed25519".into(),
                key_id: self.key_id.clone(),
                public_key: STANDARD.encode(self.key.public_key().as_ref()),
                key_version: None,
            },
            self.key.sign(pae).as_ref().to_vec(),
        )
    }
}

struct VersionedBuildEvidenceSigner {
    key: BuildEvidenceSigningKey,
}

#[async_trait]
impl IBuildEvidenceSigner for VersionedBuildEvidenceSigner {
    async fn sign(
        &self,
        _pae: &[u8],
    ) -> std::result::Result<VerifiedBuildEvidenceSignature, BuildEvidenceSigningError> {
        VerifiedBuildEvidenceSignature::new(self.key.clone(), vec![0x31; 64])
    }
}

#[tokio::test]
async fn audit_export_composition_preserves_external_signing_key_versions() {
    let key = Ed25519KeyPair::from_seed_unchecked(&[0x71; 32]).expect("Ed25519 key");
    let public_key = key.public_key().as_ref();
    let signer = BuildEvidenceAuditExportSigner {
        signer: Arc::new(VersionedBuildEvidenceSigner {
            key: BuildEvidenceSigningKey {
                algorithm: "ed25519".into(),
                key_id: format!("sha256:{:x}", Sha256::digest(public_key)),
                public_key: STANDARD.encode(public_key),
                key_version: Some(7),
            },
        }),
    };

    let signature = signer.sign(b"DSSEv1 1 x 0 ").await.expect("signature");
    assert_eq!(
        signature.key.key_id,
        format!("{:x}", Sha256::digest(public_key))
    );
    assert_eq!(signature.key.key_version, Some(7));
    assert_eq!(signature.signature, vec![0x31; 64]);
}

#[tokio::test]
async fn audit_export_composition_reloads_one_purpose_separated_local_key() {
    let root = tempfile::tempdir().expect("security state directory");
    let mut config = config();
    config.security.state_dir = root.path().to_string_lossy().into_owned();
    let pae = b"DSSEv1 1 x 0 ";

    let first = audit_export_signer(&config, None)
        .await
        .expect("first audit export signer")
        .sign(pae)
        .await
        .expect("first signature");
    let second = audit_export_signer(&config, None)
        .await
        .expect("reloaded audit export signer")
        .sign(pae)
        .await
        .expect("reloaded signature");

    assert_eq!(first.key, second.key);
    assert_eq!(first.key.key_version, None);
    assert!(root.path().join("audit-export/signing-key.pk8").is_file());
    let public_key = STANDARD
        .decode(&first.key.public_key)
        .expect("audit export public key");
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public_key)
        .verify(pae, &first.signature)
        .expect("first audit export signature");
    ring::signature::UnparsedPublicKey::new(
        &ring::signature::ED25519,
        STANDARD
            .decode(&second.key.public_key)
            .expect("reloaded audit export public key"),
    )
    .verify(pae, &second.signature)
    .expect("reloaded audit export signature");
}

mod agent_checkpoint_support;
mod agent_execution_tests;
mod api_contract_tests;
mod application_tests;
mod asset_catalog_tests;
mod asset_git_support;
mod asset_git_tests;
mod audit_tests;
mod build_tests;
mod connector_tests;
mod developer_workflow_tests;
mod durable_cell_tests;
mod execution_tests;
mod forms_tests;
mod management_mcp_tests;
mod mcp_credential_tests;
mod notification_tests;
mod oidc_tests;
mod ontology_tests;
mod operation_tests;
mod platform_tests;
mod plugin_tests;
mod project_attribution_tests;
mod recipient_contact_tests;
mod route_tests;
mod search_tests;
mod secret_tests;
mod security_tests;
mod source_lifecycle_tests;
mod source_private_tests;
mod source_subscription_tests;
mod source_tests;
mod user_file_tests;
mod workflow_tests;
mod workload_tests;

use asset_git_support::UnavailableAssetStore;

const BOOTSTRAP_TOKEN: &str = "test-bootstrap-credential-0123456789abcdef";
const ADMIN_TOKEN: &str = "a3s_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROJECT_TOKEN: &str = "a3s_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const EXPIRING_TOKEN: &str = "a3s_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const SOURCE_TOKEN: &str = "a3s_dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const FORM_TOKEN: &str = "a3s_1111111111111111111111111111111111111111111111111111111111111111";
const TOKEN_MANAGER_TOKEN: &str =
    "a3s_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const SERVICE_MEMBER_TOKEN: &str =
    "a3s_ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const PRIVILEGE_ESCALATION_TOKEN: &str =
    "a3s_0000000000000000000000000000000000000000000000000000000000000000";
const AUDIT_MEMBER_TOKEN: &str =
    "a3s_2222222222222222222222222222222222222222222222222222222222222222";
const GITHUB_WEBHOOK_SECRET: &str = "github-webhook-test-secret-0123456789abcdef";

struct TestCertificateAuthority;

struct TestLogChunkStore;

struct TestSecretEncryption;

struct TestSourceResolver;

struct UnavailableBuildPlanSourceLayout;

struct TestOciArtifactResolver;

struct TestGithubAppAuthorization;

struct UnavailableMcpRoutePolicyRepository;

struct UnavailableUserFileObjectStore;

#[async_trait]
impl IUserFileObjectStore for UnavailableUserFileObjectStore {
    async fn put(
        &self,
        _reference: &UserFileContentReference,
        _reader: UserFileObjectReader,
    ) -> std::result::Result<UserFileObjectWrite, UserFileObjectError> {
        Err(UserFileObjectError::Unavailable(
            "UserFile object writes are unavailable in this fixture".into(),
        ))
    }

    async fn verify(
        &self,
        _reference: &UserFileContentReference,
    ) -> std::result::Result<(), UserFileObjectError> {
        Err(UserFileObjectError::Unavailable(
            "UserFile object verification is unavailable in this fixture".into(),
        ))
    }
}

#[async_trait]
impl IBuildPlanSourceLayoutPort for UnavailableBuildPlanSourceLayout {
    async fn acquire(
        &self,
        request: BuildPlanSourceLayoutRequest,
    ) -> std::result::Result<
        Option<crate::modules::developer_workflows::domain::SourceLayoutSnapshot>,
        BuildPlanSourceLayoutError,
    > {
        request
            .validate()
            .map_err(BuildPlanSourceLayoutError::Invalid)?;
        Ok(None)
    }
}

#[async_trait::async_trait]
impl IOciArtifactResolver for TestOciArtifactResolver {
    async fn resolve(
        &self,
        reference: &OciArtifactReference,
        _registry_credential: Option<&OciRegistryCredentialReference>,
    ) -> std::result::Result<OciArtifact, OciArtifactResolutionError> {
        reference
            .validate()
            .map_err(OciArtifactResolutionError::InvalidReference)?;
        let repository = reference
            .repository()
            .map_err(OciArtifactResolutionError::InvalidReference)?;
        let digest = reference
            .expected_digest
            .clone()
            .or_else(|| reference.bound_digest().ok().flatten().map(str::to_owned))
            .ok_or_else(|| {
                OciArtifactResolutionError::InvalidReference(
                    "test OCI reference must pin a digest".into(),
                )
            })?;
        let artifact = OciArtifact {
            uri: format!("oci://{repository}@{digest}"),
            digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        };
        artifact
            .validate()
            .map_err(OciArtifactResolutionError::Protocol)?;
        Ok(artifact)
    }
}

struct EmptyWorkflowRunHistoryReader;

struct InputWorkflowRunVariableReader;

struct TestPluginRegistryEnrollmentAuthorizer;

struct UnavailablePluginRegistryCatalog;

type HumanTaskChangeReplayMap =
    std::collections::BTreeMap<(String, String), (String, OrganizationId, HumanTaskId)>;
type HumanTaskDecisionReplayMap =
    std::collections::BTreeMap<(String, String), (String, HumanTaskDecisionRecord)>;

#[derive(Default)]
struct TestHumanTaskRepository {
    records: std::sync::RwLock<Vec<HumanTaskRecord>>,
    change_replays: std::sync::RwLock<HumanTaskChangeReplayMap>,
    decision_replays: std::sync::RwLock<HumanTaskDecisionReplayMap>,
}

impl TestHumanTaskRepository {
    fn new(records: Vec<HumanTaskRecord>) -> Self {
        Self {
            records: std::sync::RwLock::new(records),
            change_replays: std::sync::RwLock::new(std::collections::BTreeMap::new()),
            decision_replays: std::sync::RwLock::new(std::collections::BTreeMap::new()),
        }
    }

    fn read_records(
        &self,
    ) -> std::result::Result<std::sync::RwLockReadGuard<'_, Vec<HumanTaskRecord>>, RepositoryError>
    {
        self.records
            .read()
            .map_err(|_| RepositoryError::Storage("HumanTask test fixture lock poisoned".into()))
    }

    fn insert(&self, record: HumanTaskRecord) -> std::result::Result<(), RepositoryError> {
        self.records
            .write()
            .map_err(|_| RepositoryError::Storage("HumanTask test fixture lock poisoned".into()))?
            .push(record);
        Ok(())
    }
}

#[async_trait::async_trait]
impl IHumanTaskRepository for TestHumanTaskRepository {
    async fn create_from_hook(
        &self,
        _write: CreateHumanTaskWrite,
    ) -> std::result::Result<IdempotentWrite<HumanTaskRecord>, RepositoryError> {
        Err(test_human_task_operation_unavailable())
    }

    async fn find_task(
        &self,
        organization_id: OrganizationId,
        human_task_id: HumanTaskId,
    ) -> std::result::Result<Option<HumanTaskRecord>, RepositoryError> {
        Ok(self
            .read_records()?
            .iter()
            .find(|record| {
                record.task.organization_id == organization_id && record.task.id == human_task_id
            })
            .cloned())
    }

    async fn list_tasks(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        status: Option<HumanTaskStatus>,
        limit: usize,
    ) -> std::result::Result<Vec<HumanTaskRecord>, RepositoryError> {
        Ok(self
            .read_records()?
            .iter()
            .filter(|record| {
                record.task.organization_id == organization_id
                    && record.task.project_id == project_id
                    && status.is_none_or(|status| record.task.status == status)
            })
            .take(limit)
            .cloned()
            .collect())
    }

    async fn pending_expirations(
        &self,
        expired_at: DateTime<Utc>,
        limit: usize,
    ) -> std::result::Result<Vec<HumanTaskRecord>, RepositoryError> {
        let mut records = self
            .read_records()?
            .iter()
            .filter(|record| {
                !record.task.status.is_terminal()
                    && record
                        .task
                        .expires_at
                        .is_some_and(|expires| expires <= expired_at)
            })
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by_key(|record| {
            (
                record.task.expires_at,
                record.task.organization_id,
                record.task.id,
            )
        });
        records.truncate(limit);
        Ok(records)
    }

    async fn pending_parent_cancellations(
        &self,
        _limit: usize,
    ) -> std::result::Result<Vec<HumanTaskRecord>, RepositoryError> {
        Ok(Vec::new())
    }

    async fn replay_change(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> std::result::Result<Option<HumanTaskRecord>, RepositoryError> {
        let (organization_id, human_task_id) = {
            let replays = self.change_replays.read().map_err(|_| {
                RepositoryError::Storage("HumanTask test fixture lock poisoned".into())
            })?;
            let Some((request_digest, organization_id, human_task_id)) =
                replays.get(&(idempotency.scope.clone(), idempotency.key.clone()))
            else {
                return Ok(None);
            };
            if request_digest != &idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            (*organization_id, *human_task_id)
        };
        self.find_task(organization_id, human_task_id).await
    }

    async fn change_task(
        &self,
        write: ChangeHumanTaskWrite,
    ) -> std::result::Result<IdempotentWrite<HumanTaskRecord>, RepositoryError> {
        write.record.validate().map_err(RepositoryError::Storage)?;
        if let Some(record) = self.replay_change(&write.idempotency).await? {
            return Ok(IdempotentWrite {
                value: record,
                replayed: true,
            });
        }
        let mut records = self
            .records
            .write()
            .map_err(|_| RepositoryError::Storage("HumanTask test fixture lock poisoned".into()))?;
        let current = records
            .iter_mut()
            .find(|record| {
                record.task.organization_id == write.record.task.organization_id
                    && record.task.id == write.record.task.id
            })
            .ok_or(RepositoryError::NotFound)?;
        let valid_transition = current.task.aggregate_version == write.expected_version
            && write.record.task.aggregate_version == write.expected_version.saturating_add(1)
            && match (current.task.status, write.record.task.status) {
                (HumanTaskStatus::Ready, HumanTaskStatus::Claimed) => {
                    write.record.task.claimed_by == Some(write.actor_principal_id)
                }
                (HumanTaskStatus::Claimed, HumanTaskStatus::Ready) => {
                    current.task.claimed_by == Some(write.actor_principal_id)
                }
                _ => false,
            };
        if !valid_transition {
            return Err(RepositoryError::Conflict(
                "HumanTask transition conflicts with stored test state".into(),
            ));
        }
        *current = write.record.clone();
        self.change_replays
            .write()
            .map_err(|_| RepositoryError::Storage("HumanTask test fixture lock poisoned".into()))?
            .insert(
                (write.idempotency.scope, write.idempotency.key),
                (
                    write.idempotency.request_digest,
                    write.record.task.organization_id,
                    write.record.task.id,
                ),
            );
        Ok(IdempotentWrite {
            value: write.record,
            replayed: false,
        })
    }

    async fn decide_task(
        &self,
        write: DecideHumanTaskWrite,
    ) -> std::result::Result<IdempotentWrite<HumanTaskDecisionRecord>, RepositoryError> {
        write.record.validate().map_err(RepositoryError::Storage)?;
        if let Some(record) = self.replay_decision(&write.idempotency).await? {
            return Ok(IdempotentWrite {
                value: record,
                replayed: true,
            });
        }
        let mut records = self
            .records
            .write()
            .map_err(|_| RepositoryError::Storage("HumanTask test fixture lock poisoned".into()))?;
        let current = records
            .iter_mut()
            .find(|record| {
                record.task.organization_id == write.record.task.task.organization_id
                    && record.task.id == write.record.task.task.id
            })
            .ok_or(RepositoryError::NotFound)?;
        let completes_claim = current.task.status == HumanTaskStatus::Claimed
            && current.task.claimed_by == Some(write.actor_principal_id)
            && write.record.task.task.status == HumanTaskStatus::Completed;
        let expires_non_terminal = !current.task.status.is_terminal()
            && write.record.task.task.status == HumanTaskStatus::Expired
            && write.record.decision.outcome == WorkflowDecisionOutcome::Expire;
        if current.task.aggregate_version != write.expected_version
            || write.record.task.task.aggregate_version != write.expected_version.saturating_add(1)
            || (!completes_claim && !expires_non_terminal)
        {
            return Err(RepositoryError::Conflict(
                "HumanTask decision conflicts with stored test state".into(),
            ));
        }
        *current = write.record.task.clone();
        self.decision_replays
            .write()
            .map_err(|_| RepositoryError::Storage("HumanTask test fixture lock poisoned".into()))?
            .insert(
                (write.idempotency.scope, write.idempotency.key),
                (write.idempotency.request_digest, write.record.clone()),
            );
        Ok(IdempotentWrite {
            value: write.record,
            replayed: false,
        })
    }

    async fn replay_decision(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> std::result::Result<Option<HumanTaskDecisionRecord>, RepositoryError> {
        let replays = self
            .decision_replays
            .read()
            .map_err(|_| RepositoryError::Storage("HumanTask test fixture lock poisoned".into()))?;
        let Some((request_digest, record)) =
            replays.get(&(idempotency.scope.clone(), idempotency.key.clone()))
        else {
            return Ok(None);
        };
        if request_digest != &idempotency.request_digest {
            return Err(RepositoryError::IdempotencyConflict);
        }
        Ok(Some(record.clone()))
    }

    async fn find_decision(
        &self,
        organization_id: OrganizationId,
        workflow_decision_id: WorkflowDecisionId,
    ) -> std::result::Result<Option<HumanTaskDecisionRecord>, RepositoryError> {
        Ok(self
            .decision_replays
            .read()
            .map_err(|_| RepositoryError::Storage("HumanTask test fixture lock poisoned".into()))?
            .values()
            .find(|(_, record)| {
                record.task.task.organization_id == organization_id
                    && record.decision.id == workflow_decision_id
            })
            .map(|(_, record)| record.clone()))
    }

    async fn claim_resume_deliveries(
        &self,
        _owner: Uuid,
        _limit: usize,
        _claimed_at: DateTime<Utc>,
        _lease_duration: std::time::Duration,
    ) -> std::result::Result<Vec<HumanTaskResumeDelivery>, RepositoryError> {
        Ok(Vec::new())
    }

    async fn retry_resume_delivery(
        &self,
        _organization_id: OrganizationId,
        _workflow_decision_id: WorkflowDecisionId,
        _owner: Uuid,
        _error: &str,
        _failed_at: DateTime<Utc>,
        _retry_after: std::time::Duration,
    ) -> std::result::Result<(), RepositoryError> {
        Err(test_human_task_operation_unavailable())
    }

    async fn conflict_resume_delivery(
        &self,
        _organization_id: OrganizationId,
        _workflow_decision_id: WorkflowDecisionId,
        _owner: Uuid,
        _error: &str,
        _conflicted_at: DateTime<Utc>,
    ) -> std::result::Result<(), RepositoryError> {
        Err(test_human_task_operation_unavailable())
    }

    async fn record_resume_receipt(
        &self,
        _organization_id: OrganizationId,
        _workflow_decision_id: WorkflowDecisionId,
        _owner: Uuid,
        _receipt: FlowResumeReceipt,
        _recorded_at: DateTime<Utc>,
    ) -> std::result::Result<HumanTaskDecisionRecord, RepositoryError> {
        Err(test_human_task_operation_unavailable())
    }
}

fn test_human_task_operation_unavailable() -> RepositoryError {
    RepositoryError::Storage("HumanTask test fixture operation is unavailable".into())
}

#[derive(Default)]
struct TestRuntimeRepositories {
    executions: Option<Arc<InMemoryExecutionRepository>>,
    operations: Option<Arc<InMemoryOperationRepository>>,
    human_tasks: Option<Arc<TestHumanTaskRepository>>,
    audit_records: Option<Arc<InMemoryAuditRecordRepository>>,
    security_investigations: Option<Arc<InMemoryGatewayRoutePolicyTimelineRepository>>,
    notifications: Option<Arc<crate::modules::notifications::InMemoryNotificationRepository>>,
    oidc_provider: Option<Arc<dyn IOidcProviderService>>,
    connector_profiles: Option<Arc<InMemoryConnectorProfileRepository>>,
    connector_execution: Option<Arc<InMemoryConnectorExecutionRepository>>,
    user_files: Option<Arc<InMemoryUserFileRepository>>,
    user_file_objects: Option<Arc<dyn IUserFileObjectStore>>,
}

#[async_trait::async_trait]
impl IPluginRegistryEnrollmentAuthorizer for TestPluginRegistryEnrollmentAuthorizer {
    async fn authorize_enrollment(
        &self,
        _organization_id: OrganizationId,
        _actor_id: crate::modules::shared_kernel::domain::PrincipalId,
    ) -> std::result::Result<(), PluginRegistryEnrollmentAuthorizationError> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl IPluginRegistryCatalog for UnavailablePluginRegistryCatalog {
    async fn refresh(
        &self,
        _registry: &PluginRegistry,
    ) -> std::result::Result<VerifiedRegistryMetadata, PluginRegistryCatalogError> {
        Err(unavailable_plugin_catalog())
    }

    async fn search(
        &self,
        _registry: &PluginRegistry,
        _host: &PluginCatalogHost,
        _search: &PluginCatalogSearch,
    ) -> std::result::Result<PluginCatalogPage, PluginRegistryCatalogError> {
        Err(unavailable_plugin_catalog())
    }

    async fn search_cached(
        &self,
        _registry: &PluginRegistry,
        _host: &PluginCatalogHost,
        _search: &PluginCatalogSearch,
    ) -> std::result::Result<PluginCatalogPage, PluginRegistryCatalogError> {
        Err(unavailable_plugin_catalog())
    }

    async fn inspect(
        &self,
        _registry: &PluginRegistry,
        _host: &PluginCatalogHost,
        _package_id: &str,
        _version: Option<&str>,
        _channel: Option<PluginReleaseChannel>,
    ) -> std::result::Result<PluginCatalogInspection, PluginRegistryCatalogError> {
        Err(unavailable_plugin_catalog())
    }

    async fn inspect_cached(
        &self,
        _registry: &PluginRegistry,
        _host: &PluginCatalogHost,
        _package_id: &str,
        _version: Option<&str>,
        _channel: Option<PluginReleaseChannel>,
    ) -> std::result::Result<PluginCatalogInspection, PluginRegistryCatalogError> {
        Err(unavailable_plugin_catalog())
    }
}

fn unavailable_plugin_catalog() -> PluginRegistryCatalogError {
    PluginRegistryCatalogError::Use {
        code: "fixture.plugin_catalog_unavailable".into(),
    }
}

#[async_trait::async_trait]
impl IWorkflowRunHistoryReader for EmptyWorkflowRunHistoryReader {
    async fn read(
        &self,
        _flow_run_id: &str,
        _after_sequence: u64,
        _limit: usize,
    ) -> std::result::Result<WorkflowRunHistoryPage, String> {
        Ok(WorkflowRunHistoryPage {
            events: Vec::new(),
            next_sequence: None,
        })
    }
}

#[async_trait::async_trait]
impl IWorkflowRunVariableReader for InputWorkflowRunVariableReader {
    async fn inspect(
        &self,
        record: &WorkflowRunRecord,
    ) -> std::result::Result<WorkflowRunVariableInspection, String> {
        crate::modules::workflow::domain::inspect_workflow_run_variables(
            record,
            record.run.last_flow_sequence,
            record.run.updated_at,
            &std::collections::BTreeMap::new(),
        )
    }
}

#[async_trait::async_trait]
impl IMcpRoutePolicyRepository for UnavailableMcpRoutePolicyRepository {
    async fn mutate_mcp_route_policy(
        &self,
        _write: MutateMcpRoutePolicyWrite,
    ) -> std::result::Result<McpRoutePolicyWrite, RepositoryError> {
        Err(RepositoryError::Storage(
            "MCP route policies are unavailable in this application fixture".into(),
        ))
    }

    async fn find_mcp_route_policy(
        &self,
        _organization_id: OrganizationId,
        _route_id: RouteId,
    ) -> std::result::Result<Option<McpRoutePolicy>, RepositoryError> {
        Ok(None)
    }

    async fn list_mcp_route_policies(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
    ) -> std::result::Result<Vec<McpRoutePolicy>, RepositoryError> {
        Ok(Vec::new())
    }

    async fn list_active_mcp_route_policies_for_gateway(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        _gateway_scope_id: GatewayScopeId,
        _active_at: DateTime<Utc>,
    ) -> std::result::Result<Vec<McpRoutePolicy>, RepositoryError> {
        Ok(Vec::new())
    }
}

#[async_trait::async_trait]
impl ISourceResolver for TestSourceResolver {
    async fn resolve(
        &self,
        request: &SourceResolutionRequest,
        _credential: Option<&SourceProviderCredential>,
    ) -> std::result::Result<ResolvedSource, SourceResolutionError> {
        let commit_sha = match &request.reference {
            GitReference::Commit(commit_sha) => commit_sha.clone(),
            GitReference::Branch(value) if value == "main" => {
                crate::modules::sources::domain::GitCommitSha::parse("a".repeat(40))
                    .map_err(SourceResolutionError::Protocol)?
            }
            GitReference::Tag(value) if value == "v1.0.0" => {
                crate::modules::sources::domain::GitCommitSha::parse("b".repeat(40))
                    .map_err(SourceResolutionError::Protocol)?
            }
            _ => return Err(SourceResolutionError::Unavailable),
        };
        Ok(ResolvedSource {
            repository: request.repository.clone(),
            commit_sha,
        })
    }
}

#[async_trait::async_trait]
impl IGithubAppAuthorizationService for TestGithubAppAuthorization {
    fn installation_url(
        &self,
        state: &str,
    ) -> std::result::Result<String, GithubAppAuthorizationError> {
        Ok(format!(
            "https://github.test/apps/a3s-cloud-test/installations/new?state={state}"
        ))
    }

    fn authorization_url(
        &self,
        state: &str,
        pkce_challenge: &str,
    ) -> std::result::Result<String, GithubAppAuthorizationError> {
        Ok(format!(
            "https://github.test/login/oauth/authorize?client_id=Iv1.test-client&state={state}&code_challenge={pkce_challenge}&code_challenge_method=S256"
        ))
    }

    async fn verify_installation(
        &self,
        request: GithubInstallationVerificationRequest,
    ) -> std::result::Result<VerifiedGithubInstallation, GithubAppAuthorizationError> {
        if request.code.as_str() != "valid-code" {
            return Err(GithubAppAuthorizationError::Rejected);
        }
        if request.pkce_verifier.len() != 43 || request.installation_id.as_u64() != 42 {
            return Err(GithubAppAuthorizationError::Forbidden);
        }
        Ok(VerifiedGithubInstallation {
            installation_id: request.installation_id,
            account_id: GithubAccountId::parse(100)
                .map_err(GithubAppAuthorizationError::Protocol)?,
            account_login: GithubLogin::parse("A3S-Lab")
                .map_err(GithubAppAuthorizationError::Protocol)?,
            account_kind: GithubAccountKind::Organization,
            user_id: GithubAccountId::parse(200).map_err(GithubAppAuthorizationError::Protocol)?,
            user_login: GithubLogin::parse("octocat")
                .map_err(GithubAppAuthorizationError::Protocol)?,
        })
    }
}

#[async_trait::async_trait]
impl ISecretEncryptionService for TestSecretEncryption {
    async fn encrypt(
        &self,
        plaintext: &[u8],
        context: &[u8],
    ) -> std::result::Result<EncryptedSecretValue, SecretEncryptionError> {
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
    ) -> std::result::Result<Vec<u8>, SecretEncryptionError> {
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

    async fn health(&self) -> std::result::Result<bool, SecretEncryptionError> {
        Ok(true)
    }
}

#[async_trait::async_trait]
impl crate::modules::fleet::domain::services::ILogChunkStore for TestLogChunkStore {
    async fn put(
        &self,
        _batch_id: Uuid,
        _node_id: Uuid,
        ordinal: u16,
        _report: &a3s_cloud_contracts::NodeLogChunkReport,
    ) -> std::result::Result<
        crate::modules::fleet::domain::services::StoredLogChunk,
        crate::modules::fleet::domain::services::LogChunkStoreError,
    > {
        Ok(crate::modules::fleet::domain::services::StoredLogChunk {
            object_key: format!("test/{ordinal}"),
            created: false,
        })
    }

    async fn get(
        &self,
        _object_key: &str,
        _expected_checksum: &str,
    ) -> std::result::Result<
        crate::modules::fleet::domain::services::RetrievedLogChunk,
        crate::modules::fleet::domain::services::LogChunkStoreError,
    > {
        Ok(crate::modules::fleet::domain::services::RetrievedLogChunk::Missing)
    }

    async fn remove(
        &self,
        _object_key: &str,
    ) -> std::result::Result<(), crate::modules::fleet::domain::services::LogChunkStoreError> {
        Ok(())
    }

    async fn health(
        &self,
    ) -> std::result::Result<bool, crate::modules::fleet::domain::services::LogChunkStoreError>
    {
        Ok(true)
    }
}

#[async_trait::async_trait]
impl ICertificateAuthority for TestCertificateAuthority {
    async fn issue(
        &self,
        request: NodeCertificateRequest,
    ) -> std::result::Result<NodeCertificate, CertificateAuthorityError> {
        NodeCertificate::new(
            request.certificate_id,
            request.node_id,
            NodeCertificateMaterial {
                serial_number: request.certificate_id.to_string(),
                fingerprint: format!("sha256:{:x}", Sha256::digest(request.csr_pem.as_bytes())),
                certificate_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n".into(),
                ca_bundle_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n".into(),
                issued_at: request.issued_at,
                expires_at: request.expires_at,
            },
        )
        .map_err(CertificateAuthorityError::InvalidRequest)
    }

    async fn revoke(
        &self,
        _certificate: &NodeCertificate,
    ) -> std::result::Result<(), CertificateAuthorityError> {
        Ok(())
    }

    async fn health(&self) -> std::result::Result<bool, CertificateAuthorityError> {
        Ok(true)
    }
}

#[test]
fn application_test_configuration_satisfies_production_policy() {
    config()
        .validate()
        .expect("application test configuration must remain valid");
}

fn config() -> CloudConfig {
    CloudConfig {
        server: ServerConfig {
            host: "127.0.0.1".into(),
            port: 8080,
            role: ProcessRole::All,
        },
        node_control: NodeControlConfig {
            host: "127.0.0.1".into(),
            port: 8443,
            server_name: "localhost".into(),
            certificate_file: ".a3s/test-security/node-control/server.pem".into(),
            private_key_file: ".a3s/test-security/node-control/server-key.pem".into(),
            client_ca_file: ".a3s/test-security/node-ca/ca.pem".into(),
            max_request_bytes: 20 * 1024 * 1024,
            tls_handshake_timeout_ms: 5_000,
            request_body_timeout_ms: 10_000,
        },
        artifacts: ArtifactTransferConfig {
            max_blob_bytes: 1024 * 1024 * 1024,
            transfer_timeout_ms: 900_000,
        },
        assets: AssetsConfig {
            repository_dir: ".a3s/test-asset-repositories".into(),
            git_command_timeout_ms: 10_000,
            write_lease_ms: 30_000,
            repository_quota_bytes: 1024 * 1024 * 1024,
            max_rpc_body_bytes: 64 * 1024 * 1024,
            backup_max_bytes: 1024 * 1024 * 1024,
        },
        objects: ObjectStorageConfig {
            provider: ObjectStorageProviderKind::Local,
            local_dir: ".a3s/test-objects".into(),
            endpoint: String::new(),
            region: "us-east-1".into(),
            bucket: "a3s-cloud-objects".into(),
            prefix: "cloud".into(),
            access_key_env: "A3S_CLOUD_S3_ACCESS_KEY_ID".into(),
            secret_key_env: "A3S_CLOUD_S3_SECRET_ACCESS_KEY".into(),
            session_token_env: String::new(),
            allow_http: false,
            virtual_hosted_style: false,
            request_timeout_ms: 30_000,
            connect_timeout_ms: 5_000,
            retry_timeout_ms: 60_000,
            max_retries: 3,
        },
        postgres: PostgresConfig {
            serving_role: "a3s_cloud_serving".into(),
            serving_url_env: "A3S_CLOUD_POSTGRES_URL".into(),
            migration_url_env: "A3S_CLOUD_POSTGRES_MIGRATION_URL".into(),
            max_connections: 4,
        },
        auth: AuthConfig {
            bootstrap_token_env: "A3S_CLOUD_BOOTSTRAP_TOKEN".into(),
            oidc_providers: Vec::new(),
        },
        events: EventsConfig {
            provider: EventProviderKind::Memory,
            nats_url_env: "A3S_CLOUD_NATS_URL".into(),
            stream_name: "A3S_CLOUD_EVENTS".into(),
            batch_size: 100,
            poll_interval_ms: 250,
            lease_ms: 10_000,
            publish_timeout_ms: 3_000,
            retry_initial_ms: 500,
            retry_max_ms: 30_000,
        },
        smtp: SmtpConfig {
            provider: SmtpProviderKind::Disabled,
            host: "smtp.example.test".into(),
            port: 465,
            tls: SmtpTlsMode::Implicit,
            hello_name: "cloud.example.test".into(),
            ca_certificate_file: String::new(),
            username_env: "A3S_CLOUD_SMTP_USERNAME".into(),
            password_env: "A3S_CLOUD_SMTP_PASSWORD".into(),
            sender: "no-reply@example.test".into(),
            connect_timeout_ms: 5_000,
            command_timeout_ms: 10_000,
            reservation_lease_ms: 60_000,
        },
        operations: OperationsConfig {
            reconcile_interval_ms: 1_000,
            lease_ms: 5_000,
        },
        human_tasks: HumanTasksConfig {
            coordination_poll_interval_ms: 100,
            coordination_batch_size: 100,
            resume_poll_interval_ms: 100,
            resume_batch_size: 100,
            resume_lease_ms: 5_000,
            flow_operation_timeout_ms: 1_000,
            retry_initial_ms: 100,
            retry_max_ms: 5_000,
        },
        deployments: DeploymentsConfig {
            reconcile_interval_ms: 1_000,
            command_ttl_ms: 10_000,
            runtime_apply_timeout_ms: 5_000,
            observation_poll_ms: 10,
            convergence_timeout_ms: 20_000,
            runtime_stop_timeout_ms: 5_000,
            cleanup_poll_ms: 10,
            cleanup_timeout_ms: 20_000,
        },
        executions: crate::config::ExecutionsConfig {
            reconcile_interval_ms: 1_000,
            command_ttl_ms: 900_000,
            observation_poll_ms: 10,
            convergence_timeout_ms: 20_000,
            cleanup_timeout_ms: 20_000,
            checkpoint_object_reconcile_interval_ms: 1_000,
            checkpoint_object_capture_lease_ms: 120_000,
            checkpoint_object_orphan_grace_ms: 600_000,
            checkpoint_object_cleanup_lease_ms: 120_000,
            checkpoint_object_reconcile_batch_size: 100,
        },
        builds: BuildsConfig {
            reconcile_interval_ms: 1_000,
            input_staging_dir: ".a3s/test-build-input".into(),
            input_max_entries: 10_000,
            input_max_bytes: 128 * 1024 * 1024,
            output_staging_dir: ".a3s/test-build-output".into(),
            output_max_entries: 10_000,
            output_max_expanded_bytes: 256 * 1024 * 1024,
            oci_max_blobs: 1_000,
            oci_max_bytes: 256 * 1024 * 1024,
            command_ttl_ms: 10_000,
            execution_timeout_ms: 5_000,
            observation_poll_ms: 10,
            convergence_timeout_ms: 20_000,
            cleanup_timeout_ms: 20_000,
            output_max_bytes: 128 * 1024 * 1024,
            cache_max_bytes: 128 * 1024 * 1024,
        },
        registry: RegistryConfig {
            request_timeout_ms: 10_000,
            insecure_hosts: vec!["127.0.0.1:5000".into()],
            publication_registry: "127.0.0.1:5000".into(),
            publication_repository_prefix: "a3s-cloud/builds".into(),
            publication_credential_env: String::new(),
            publication_allow_anonymous: true,
            publication_timeout_ms: 60_000,
        },
        sources: SourcesConfig {
            github_request_timeout_ms: 10_000,
            github_webhook_secret_env: "A3S_CLOUD_GITHUB_WEBHOOK_SECRET".into(),
            github_webhook_max_body_bytes: 1024 * 1024,
            github_app_enabled: true,
            github_app_slug: "a3s-cloud-test".into(),
            github_app_client_id: "Iv1.test-client".into(),
            github_app_client_secret_env: "A3S_CLOUD_GITHUB_APP_CLIENT_SECRET".into(),
            github_app_private_key_env: "A3S_CLOUD_GITHUB_APP_PRIVATE_KEY".into(),
            github_app_callback_url:
                "https://cloud.example.test/api/v1/source-connections/github/callback".into(),
            github_connection_state_ttl_ms: 600_000,
            github_authority_reconcile_interval_ms: 10_000,
            github_authority_poll_interval_ms: 300_000,
            github_authority_retry_initial_ms: 1_000,
            github_authority_retry_max_ms: 60_000,
            github_authority_batch_size: 100,
            checkout_dir: ".a3s/test-source-checkouts".into(),
            checkout_timeout_ms: 10_000,
            checkout_max_files: 10_000,
            checkout_max_bytes: 64 * 1024 * 1024,
            allowed_repositories: vec!["https://github.com/A3S-Lab/Cloud".into()],
            denied_repositories: Vec::new(),
        },
        logs: LogsConfig {
            retention_ms: 60_000,
            retention_poll_ms: 1_000,
            retention_batch_size: 16,
            tombstone_retention_ms: 300_000,
            tombstone_compaction_poll_ms: 10_000,
            tombstone_compaction_batch_size: 64,
        },
        audit: AuditConfig {
            retention_ms: 7_776_000_000,
            retention_poll_ms: 60_000,
            retention_organization_batch_size: 32,
            retention_record_batch_size: 256,
        },
        edge: EdgeConfig {
            entrypoint_address: "0.0.0.0:8081".into(),
            management_address: "127.0.0.1:9090".into(),
            management_path_prefix: "/api/gateway".into(),
            management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
            domain_verification_timeout_ms: 5_000,
            certificate_directory: "/tmp/a3s-cloud-test/gateway/certificates".into(),
            managed_state_file: "/tmp/a3s-cloud-test/gateway/managed-snapshot.json".into(),
            certificate_ttl_ms: 2_592_000_000,
            certificate_renewal_window_ms: 604_800_000,
            snapshot_renewal_window_ms: 21_600_000,
            certificate_reconciliation_interval_ms: 60_000,
            upstream_request_timeout_ms: 30_000,
            command_ttl_ms: 10_000,
        },
        fleet: FleetConfig {
            heartbeat_interval_ms: 1_000,
            heartbeat_timeout_ms: 5_000,
            protocol_session_ttl_ms: 300_000,
            command_long_poll_ms: 1_000,
            command_lease_ms: 5_000,
            certificate_ttl_ms: 3_600_000,
            certificate_rotation_window_ms: 900_000,
        },
        security: SecurityConfig {
            profile: SecurityProfile::Development,
            state_dir: ".a3s/test-security".into(),
            certificate_authority: SecurityProviderKind::Local,
            gateway_certificate_authority: SecurityProviderKind::Local,
            key_encryption: SecurityProviderKind::Local,
            build_evidence_signing: SecurityProviderKind::Local,
            audit_export_signing: SecurityProviderKind::Local,
            recipient_contact_proof: SecurityProviderKind::Local,
            recipient_contact_proof_key_id: "recipient-contact-v1".into(),
            vault_address_env: "A3S_CLOUD_VAULT_ADDR".into(),
            vault_token_env: "A3S_CLOUD_VAULT_TOKEN".into(),
            vault_pki_mount: "pki".into(),
            vault_pki_role: "a3s-cloud-node".into(),
            vault_gateway_pki_mount: "gateway-pki".into(),
            vault_gateway_pki_role: "a3s-cloud-gateway".into(),
            vault_transit_mount: "transit".into(),
            vault_transit_key: "a3s-cloud".into(),
            vault_build_evidence_signing_key: "a3s-cloud-build-evidence".into(),
            vault_audit_export_signing_key: "a3s-cloud-audit-export".into(),
            vault_recipient_contact_proof_key: "a3s-cloud-recipient-contact-proof".into(),
            vault_timeout_ms: 5_000,
        },
    }
}

fn post_json(path: impl Into<String>, idempotency_key: &str, body: Value) -> BootRequest {
    post_json_as(path, idempotency_key, body, ADMIN_TOKEN)
}

fn post_json_as(
    path: impl Into<String>,
    idempotency_key: &str,
    body: Value,
    token: &str,
) -> BootRequest {
    BootRequest::new(HttpMethod::Post, path.into())
        .with_header("content-type", "application/json")
        .with_header("idempotency-key", idempotency_key)
        .with_header("authorization", format!("Bearer {token}"))
        .with_body(body.to_string().into_bytes())
}

fn post_acl(
    path: impl Into<String>,
    idempotency_key: &str,
    body: impl Into<Vec<u8>>,
) -> BootRequest {
    post_acl_as(path, idempotency_key, body, ADMIN_TOKEN)
}

fn post_acl_as(
    path: impl Into<String>,
    idempotency_key: &str,
    body: impl Into<Vec<u8>>,
    token: &str,
) -> BootRequest {
    BootRequest::new(HttpMethod::Post, path.into())
        .with_header("content-type", "application/vnd.a3s.acl; charset=utf-8")
        .with_header("idempotency-key", idempotency_key)
        .with_header("authorization", format!("Bearer {token}"))
        .with_body(body)
}

fn delete_as(path: impl Into<String>, idempotency_key: &str, token: &str) -> BootRequest {
    BootRequest::new(HttpMethod::Delete, path.into())
        .with_header("idempotency-key", idempotency_key)
        .with_header("authorization", format!("Bearer {token}"))
}

fn get_as(path: impl Into<String>, token: &str) -> BootRequest {
    BootRequest::new(HttpMethod::Get, path.into())
        .with_header("accept", "application/json")
        .with_header("authorization", format!("Bearer {token}"))
}

fn mcp_tool_call_as(id: u64, name: &str, arguments: Value, token: &str) -> BootRequest {
    let body = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": arguments,
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": a3s_cloud_contracts::MCP_PROTOCOL_VERSION,
                "io.modelcontextprotocol/clientInfo": {
                    "name": "a3s-cloud-test",
                    "version": "1.0.0"
                },
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }
    });
    BootRequest::new(HttpMethod::Post, "/api/v1/mcp")
        .with_header("content-type", "application/json")
        .with_header("accept", "application/json, text/event-stream")
        .with_header("authorization", format!("Bearer {token}"))
        .with_header(
            "mcp-protocol-version",
            a3s_cloud_contracts::MCP_PROTOCOL_VERSION,
        )
        .with_header("mcp-method", "tools/call")
        .with_header("mcp-name", name)
        .with_body(body.to_string().into_bytes())
}

async fn assert_resource_not_found_equivalent(
    app: &BootApplication,
    denied_request: BootRequest,
    missing_request: BootRequest,
) -> Result<()> {
    let denied = app.call(denied_request).await?;
    let missing = app.call(missing_request).await?;
    assert_eq!(denied.status(), 404);
    assert_eq!(missing.status(), 404);
    let denied = response_json(&denied)?;
    let missing = response_json(&missing)?;
    for field in ["code", "statusCode", "message", "details"] {
        assert_eq!(denied[field], missing[field]);
    }
    Ok(())
}

fn assert_mcp_not_found_equivalent(denied: &BootResponse, missing: &BootResponse) -> Result<()> {
    let denied = response_json(denied)?;
    let missing = response_json(missing)?;
    assert_eq!(denied["result"]["isError"], true);
    assert_eq!(missing["result"]["isError"], true);
    assert_eq!(denied["result"]["structuredContent"]["code"], 404);
    assert_eq!(missing["result"]["structuredContent"]["code"], 404);
    for field in ["code", "statusCode", "message", "details"] {
        assert_eq!(
            denied["result"]["structuredContent"][field],
            missing["result"]["structuredContent"][field]
        );
    }
    Ok(())
}

fn build_test_application(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
) -> Result<BootApplication> {
    build_test_application_with_secrets(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
    )
}

fn build_test_application_with_user_files(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    user_files: Arc<InMemoryUserFileRepository>,
    user_file_objects: Arc<dyn IUserFileObjectStore>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds_and_search_and_edge_with_runtime_repositories(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::new(TestSourceResolver),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(TestGithubAppAuthorization),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        Arc::new(InMemoryBuildRunRepository::new()),
        Arc::new(InMemorySearchRepository::new()),
        Arc::new(crate::modules::edge::InMemoryEdgeRepository::new()),
        None,
        None,
        TestRuntimeRepositories {
            user_files: Some(user_files),
            user_file_objects: Some(user_file_objects),
            ..TestRuntimeRepositories::default()
        },
    )
}

fn build_test_application_with_connector_repositories(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    connector_profiles: Arc<InMemoryConnectorProfileRepository>,
    connector_execution: Arc<InMemoryConnectorExecutionRepository>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds_and_search_and_edge_with_runtime_repositories(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::new(TestSourceResolver),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(TestGithubAppAuthorization),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        Arc::new(InMemoryBuildRunRepository::new()),
        Arc::new(InMemorySearchRepository::new()),
        Arc::new(crate::modules::edge::InMemoryEdgeRepository::new()),
        None,
        None,
        TestRuntimeRepositories {
            connector_profiles: Some(connector_profiles),
            connector_execution: Some(connector_execution),
            ..TestRuntimeRepositories::default()
        },
    )
}

fn build_test_application_with_oidc_provider(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    oidc_provider: Arc<dyn IOidcProviderService>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds_and_search_and_edge_with_runtime_repositories(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::new(TestSourceResolver),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(TestGithubAppAuthorization),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        Arc::new(InMemoryBuildRunRepository::new()),
        Arc::new(InMemorySearchRepository::new()),
        Arc::new(crate::modules::edge::InMemoryEdgeRepository::new()),
        None,
        None,
        TestRuntimeRepositories {
            oidc_provider: Some(oidc_provider),
            ..TestRuntimeRepositories::default()
        },
    )
}

fn build_test_application_with_edge(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    edge: Arc<crate::modules::edge::InMemoryEdgeRepository>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds_and_search_and_edge(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::new(TestSourceResolver),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(TestGithubAppAuthorization),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        Arc::new(InMemoryBuildRunRepository::new()),
        Arc::new(InMemorySearchRepository::new()),
        edge,
        None,
        None,
    )
}

fn build_test_application_with_asset_store(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    assets: Arc<UnavailableAssetStore>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds_and_search_and_edge(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::new(TestSourceResolver),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(TestGithubAppAuthorization),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        Arc::new(InMemoryBuildRunRepository::new()),
        Arc::new(InMemorySearchRepository::new()),
        Arc::new(crate::modules::edge::InMemoryEdgeRepository::new()),
        Some(assets),
        None,
    )
}

fn build_test_application_with_agent_repositories(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    assets: Arc<UnavailableAssetStore>,
    builds: Arc<InMemoryBuildRunRepository>,
    agents: Arc<InMemoryAgentRepository>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds_and_search_and_edge(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::new(TestSourceResolver),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(TestGithubAppAuthorization),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        builds,
        Arc::new(InMemorySearchRepository::new()),
        Arc::new(crate::modules::edge::InMemoryEdgeRepository::new()),
        Some(assets),
        Some(agents),
    )
}

fn build_test_application_with_execution_and_operation_repositories(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    executions: Arc<InMemoryExecutionRepository>,
    operations: Arc<InMemoryOperationRepository>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds_and_search_and_edge_with_runtime_repositories(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::new(TestSourceResolver),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(TestGithubAppAuthorization),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        Arc::new(InMemoryBuildRunRepository::new()),
        Arc::new(InMemorySearchRepository::new()),
        Arc::new(crate::modules::edge::InMemoryEdgeRepository::new()),
        None,
        None,
        TestRuntimeRepositories {
            executions: Some(executions),
            operations: Some(operations),
            ..TestRuntimeRepositories::default()
        },
    )
}

fn build_test_application_with_audit_records(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    audit_records: Arc<InMemoryAuditRecordRepository>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds_and_search_and_edge_with_runtime_repositories(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::new(TestSourceResolver),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(TestGithubAppAuthorization),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        Arc::new(InMemoryBuildRunRepository::new()),
        Arc::new(InMemorySearchRepository::new()),
        Arc::new(crate::modules::edge::InMemoryEdgeRepository::new()),
        None,
        None,
        TestRuntimeRepositories {
            audit_records: Some(audit_records),
            ..TestRuntimeRepositories::default()
        },
    )
}

fn build_test_application_with_security_investigations(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    security_investigations: Arc<InMemoryGatewayRoutePolicyTimelineRepository>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds_and_search_and_edge_with_runtime_repositories(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::new(TestSourceResolver),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(TestGithubAppAuthorization),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        Arc::new(InMemoryBuildRunRepository::new()),
        Arc::new(InMemorySearchRepository::new()),
        Arc::new(crate::modules::edge::InMemoryEdgeRepository::new()),
        None,
        None,
        TestRuntimeRepositories {
            security_investigations: Some(security_investigations),
            ..TestRuntimeRepositories::default()
        },
    )
}

fn build_test_application_with_notifications(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    notifications: Arc<crate::modules::notifications::InMemoryNotificationRepository>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds_and_search_and_edge_with_runtime_repositories(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::new(TestSourceResolver),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(TestGithubAppAuthorization),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        Arc::new(InMemoryBuildRunRepository::new()),
        Arc::new(InMemorySearchRepository::new()),
        Arc::new(crate::modules::edge::InMemoryEdgeRepository::new()),
        None,
        None,
        TestRuntimeRepositories {
            notifications: Some(notifications),
            ..TestRuntimeRepositories::default()
        },
    )
}

fn build_test_application_with_human_tasks(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    human_tasks: Arc<TestHumanTaskRepository>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds_and_search_and_edge_with_runtime_repositories(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::new(TestSourceResolver),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(TestGithubAppAuthorization),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        Arc::new(InMemoryBuildRunRepository::new()),
        Arc::new(InMemorySearchRepository::new()),
        Arc::new(crate::modules::edge::InMemoryEdgeRepository::new()),
        None,
        None,
        TestRuntimeRepositories {
            human_tasks: Some(human_tasks),
            ..TestRuntimeRepositories::default()
        },
    )
}

fn build_test_application_with_search(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    search: Arc<InMemorySearchRepository>,
) -> Result<BootApplication> {
    let search: Arc<dyn ISearchRepository> = search;
    build_test_application_with_source_dependencies_and_tokens_and_builds_and_search(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::new(TestSourceResolver),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(TestGithubAppAuthorization),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        Arc::new(InMemoryBuildRunRepository::new()),
        search,
    )
}

fn build_test_application_with_secrets(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    secrets: Arc<InMemorySecretRepository>,
) -> Result<BootApplication> {
    build_test_application_with_repositories(
        identity,
        projects,
        secrets,
        Arc::new(InMemoryWorkloadRepository::new()),
    )
}

fn build_test_application_with_repositories(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    secrets: Arc<InMemorySecretRepository>,
    workloads: Arc<InMemoryWorkloadRepository>,
) -> Result<BootApplication> {
    build_test_application_with_all_repositories(
        identity,
        projects,
        secrets,
        workloads,
        Arc::new(InMemorySourceRevisionRepository::new()),
    )
}

fn build_test_application_with_sources(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    sources: Arc<InMemorySourceRevisionRepository>,
) -> Result<BootApplication> {
    build_test_application_with_all_repositories(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        sources,
    )
}

fn build_test_application_with_all_repositories(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    secrets: Arc<InMemorySecretRepository>,
    workloads: Arc<InMemoryWorkloadRepository>,
    sources: Arc<InMemorySourceRevisionRepository>,
) -> Result<BootApplication> {
    build_test_application_with_source_resolver(
        identity,
        projects,
        secrets,
        workloads,
        sources,
        Arc::new(TestSourceResolver),
    )
}

fn build_test_application_with_external_builds(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    secrets: Arc<InMemorySecretRepository>,
    workloads: Arc<InMemoryWorkloadRepository>,
    sources: Arc<InMemorySourceRevisionRepository>,
    builds: Arc<InMemoryBuildRunRepository>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds(
        identity,
        projects,
        secrets,
        workloads,
        sources,
        Arc::new(TestSourceResolver),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(TestGithubAppAuthorization),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        builds,
    )
}

fn build_test_application_with_source_resolver(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    secrets: Arc<InMemorySecretRepository>,
    workloads: Arc<InMemoryWorkloadRepository>,
    sources: Arc<InMemorySourceRevisionRepository>,
    source_resolver: Arc<dyn ISourceResolver>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies(
        identity,
        projects,
        secrets,
        workloads,
        sources,
        source_resolver,
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(TestGithubAppAuthorization),
    )
}

fn build_test_application_with_github_connections(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    connections: Arc<InMemoryGithubConnectionRepository>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::new(TestSourceResolver),
        connections,
        Arc::new(TestGithubAppAuthorization),
    )
}

#[allow(clippy::too_many_arguments)]
fn build_test_application_with_source_dependencies(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    secrets: Arc<InMemorySecretRepository>,
    workloads: Arc<InMemoryWorkloadRepository>,
    sources: Arc<InMemorySourceRevisionRepository>,
    source_resolver: Arc<dyn ISourceResolver>,
    github_connections: Arc<InMemoryGithubConnectionRepository>,
    github_authorization: Arc<dyn IGithubAppAuthorizationService>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens(
        identity,
        projects,
        secrets,
        workloads,
        sources,
        source_resolver,
        github_connections,
        github_authorization,
        Arc::new(GithubInstallationTokenIssuer::disabled()),
    )
}

#[allow(clippy::too_many_arguments)]
fn build_test_application_with_source_dependencies_and_tokens(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    secrets: Arc<InMemorySecretRepository>,
    workloads: Arc<InMemoryWorkloadRepository>,
    sources: Arc<InMemorySourceRevisionRepository>,
    source_resolver: Arc<dyn ISourceResolver>,
    github_connections: Arc<InMemoryGithubConnectionRepository>,
    github_authorization: Arc<dyn IGithubAppAuthorizationService>,
    github_installation_tokens: Arc<dyn IGithubInstallationTokenService>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds(
        identity,
        projects,
        secrets,
        workloads,
        sources,
        source_resolver,
        github_connections,
        github_authorization,
        github_installation_tokens,
        Arc::new(InMemoryBuildRunRepository::new()),
    )
}

#[allow(clippy::too_many_arguments)]
fn build_test_application_with_source_dependencies_and_tokens_and_builds(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    secrets: Arc<InMemorySecretRepository>,
    workloads: Arc<InMemoryWorkloadRepository>,
    sources: Arc<InMemorySourceRevisionRepository>,
    source_resolver: Arc<dyn ISourceResolver>,
    github_connections: Arc<InMemoryGithubConnectionRepository>,
    github_authorization: Arc<dyn IGithubAppAuthorizationService>,
    github_installation_tokens: Arc<dyn IGithubInstallationTokenService>,
    builds: Arc<dyn IBuildRunRepository>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds_and_search(
        identity,
        projects,
        secrets,
        workloads,
        sources,
        source_resolver,
        github_connections,
        github_authorization,
        github_installation_tokens,
        builds,
        Arc::new(InMemorySearchRepository::new()),
    )
}

#[allow(clippy::too_many_arguments)]
fn build_test_application_with_source_dependencies_and_tokens_and_builds_and_search(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    secrets: Arc<InMemorySecretRepository>,
    workloads: Arc<InMemoryWorkloadRepository>,
    sources: Arc<InMemorySourceRevisionRepository>,
    source_resolver: Arc<dyn ISourceResolver>,
    github_connections: Arc<InMemoryGithubConnectionRepository>,
    github_authorization: Arc<dyn IGithubAppAuthorizationService>,
    github_installation_tokens: Arc<dyn IGithubInstallationTokenService>,
    builds: Arc<dyn IBuildRunRepository>,
    search: Arc<dyn ISearchRepository>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds_and_search_and_edge(
        identity,
        projects,
        secrets,
        workloads,
        sources,
        source_resolver,
        github_connections,
        github_authorization,
        github_installation_tokens,
        builds,
        search,
        Arc::new(crate::modules::edge::InMemoryEdgeRepository::new()),
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_test_application_with_source_dependencies_and_tokens_and_builds_and_search_and_edge(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    secrets: Arc<InMemorySecretRepository>,
    workloads: Arc<InMemoryWorkloadRepository>,
    sources: Arc<InMemorySourceRevisionRepository>,
    source_resolver: Arc<dyn ISourceResolver>,
    github_connections: Arc<InMemoryGithubConnectionRepository>,
    github_authorization: Arc<dyn IGithubAppAuthorizationService>,
    github_installation_tokens: Arc<dyn IGithubInstallationTokenService>,
    builds: Arc<dyn IBuildRunRepository>,
    search: Arc<dyn ISearchRepository>,
    edge: Arc<crate::modules::edge::InMemoryEdgeRepository>,
    test_assets: Option<Arc<UnavailableAssetStore>>,
    test_agents: Option<Arc<InMemoryAgentRepository>>,
) -> Result<BootApplication> {
    build_test_application_with_source_dependencies_and_tokens_and_builds_and_search_and_edge_with_runtime_repositories(
        identity,
        projects,
        secrets,
        workloads,
        sources,
        source_resolver,
        github_connections,
        github_authorization,
        github_installation_tokens,
        builds,
        search,
        edge,
        test_assets,
        test_agents,
        TestRuntimeRepositories::default(),
    )
}

#[allow(clippy::too_many_arguments)]
fn build_test_application_with_source_dependencies_and_tokens_and_builds_and_search_and_edge_with_runtime_repositories(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    secrets: Arc<InMemorySecretRepository>,
    workloads: Arc<InMemoryWorkloadRepository>,
    sources: Arc<InMemorySourceRevisionRepository>,
    source_resolver: Arc<dyn ISourceResolver>,
    github_connections: Arc<InMemoryGithubConnectionRepository>,
    github_authorization: Arc<dyn IGithubAppAuthorizationService>,
    github_installation_tokens: Arc<dyn IGithubInstallationTokenService>,
    builds: Arc<dyn IBuildRunRepository>,
    search: Arc<dyn ISearchRepository>,
    edge: Arc<crate::modules::edge::InMemoryEdgeRepository>,
    test_assets: Option<Arc<UnavailableAssetStore>>,
    test_agents: Option<Arc<InMemoryAgentRepository>>,
    runtime_repositories: TestRuntimeRepositories,
) -> Result<BootApplication> {
    let TestRuntimeRepositories {
        executions,
        operations,
        human_tasks,
        audit_records,
        security_investigations,
        notifications,
        oidc_provider,
        connector_profiles,
        connector_execution,
        user_files,
        user_file_objects,
    } = runtime_repositories;
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let node_control: Arc<dyn INodeControlRepository> = nodes.clone();
    let workload_port: Arc<dyn IWorkloadRepository> = workloads;
    let routes: Arc<dyn IEdgeRepository> = edge.clone();
    let mcp_credentials: Arc<dyn IMcpCredentialLifecycleRepository> = edge;
    let gateway_projector: Arc<dyn IGatewayAcknowledgementProjector> = Arc::new(
        EdgeGatewayAcknowledgementProjector::new(Arc::clone(&routes)),
    );
    let route_targets: Arc<dyn IRouteTargetReader> = Arc::new(
        WorkloadRouteTargetReader::new(
            Arc::clone(&workload_port),
            Arc::clone(&node_control),
            chrono::Duration::seconds(5),
        )
        .map_err(BootError::Internal)?,
    );
    let route_commands: Arc<dyn IGatewayCommandQueue> =
        Arc::new(FleetGatewayCommandQueue::new(Arc::clone(&node_control)));
    let source_webhooks = sources.clone();
    let source_subscriptions = sources.clone();
    let unavailable_assets =
        test_assets.unwrap_or_else(|| Arc::new(UnavailableAssetStore::default()));
    let mcp_service_profiles = Arc::new(McpServiceProfileApplicationService::new(
        unavailable_assets.clone(),
        unavailable_assets.clone(),
    ));
    let mcp_route_policies = Arc::new(McpRoutePolicyApplicationService::new(
        Arc::new(UnavailableMcpRoutePolicyRepository),
        unavailable_assets.clone(),
    ));
    let asset_catalog = Arc::new(AssetCatalogApplicationService::new(
        identity.clone(),
        unavailable_assets.clone(),
        unavailable_assets.clone(),
        unavailable_assets.clone(),
    ));
    let asset_git = Arc::new(
        AssetGitApplicationService::new(
            unavailable_assets.clone(),
            unavailable_assets.clone(),
            unavailable_assets.clone(),
            AssetGitApplicationServiceOptions {
                write_lease: std::time::Duration::from_secs(30),
                default_repository_quota_bytes: 1024 * 1024 * 1024,
                maximum_rpc_body_bytes: 64 * 1024 * 1024,
            },
        )
        .map_err(BootError::Internal)?,
    );
    let notification_repository = notifications.unwrap_or_else(|| {
        Arc::new(crate::modules::notifications::InMemoryNotificationRepository::new())
    });
    let notifications: Arc<dyn INotificationRepository> = notification_repository.clone();
    let alert_policies: Arc<dyn INotificationAlertPolicyRepository> =
        notification_repository.clone();
    let outbound_notifications: Arc<dyn IOutboundNotificationRepository> = notification_repository;
    let connector_profiles =
        connector_profiles.unwrap_or_else(|| Arc::new(InMemoryConnectorProfileRepository::new()));
    let connector_execution = connector_execution
        .unwrap_or_else(|| Arc::new(InMemoryConnectorExecutionRepository::new()));
    let source_repository_credentials: Arc<dyn ISourceRepositoryCredentialProvider> =
        Arc::new(SourceRepositoryCredentialService::new(
            github_connections.clone(),
            github_installation_tokens,
        ));
    let forms: Arc<dyn IFormRepository> = Arc::new(InMemoryFormRepository::new());
    let form_semantic_core: Arc<dyn IFormSemanticCore> = Arc::new(NativeFormSemanticCore::new());
    let human_task_forms: Arc<dyn IHumanTaskFormPort> = Arc::new(FormsHumanTaskFormAdapter::new(
        Arc::clone(&forms),
        Arc::clone(&form_semantic_core),
    ));
    build_management_application_with_health(
        config(),
        ManagementApplicationDependencies {
            management: ManagementSurfaceDependencies {
                oidc_provider: oidc_provider.unwrap_or(Arc::new(
                    OpenIdConnectProviderService::new(&[]).map_err(BootError::Internal)?,
                )),
                plugin_trust_roots: Arc::new(
                    PluginTrustRootObjectStore::in_memory(MAX_BOOTSTRAP_ROOT_BYTES)
                        .map_err(|error| BootError::Internal(error.to_string()))?,
                ),
                plugin_catalog: Arc::new(UnavailablePluginRegistryCatalog),
                asset_catalog,
                mcp_service_profiles,
                mcp_route_policies,
                asset_git,
                github_authorization,
                source_resolver,
                github_source_discovery: Arc::new(GithubInstallationTokenIssuer::disabled()),
                source_repository_credentials,
                developer_workflow_source_layouts: Arc::new(UnavailableBuildPlanSourceLayout),
                source_webhook_verifier: Arc::new(
                    GithubWebhookVerifier::for_test(GITHUB_WEBHOOK_SECRET, 1024 * 1024)
                        .map_err(BootError::Internal)?,
                ),
                domain_verifier: Arc::new(LocalDomainOwnershipVerifier),
                gateway_projector,
                certificate_authority: Arc::new(TestCertificateAuthority),
                bootstrap_credential: BootstrapCredential::new(BOOTSTRAP_TOKEN)
                    .map_err(BootError::Internal)?,
            },
            identity_bootstrap: identity.clone(),
            organizations: identity.clone(),
            api_tokens: identity.clone(),
            memberships: identity.clone(),
            membership_invitations: identity.clone(),
            resource_grants: identity.clone(),
            oidc_identity: identity.clone(),
            recipient_contacts: identity.clone(),
            recipient_contact_proof: Arc::new(
                HmacRecipientContactProofService::new(
                    RecipientContactSigningKeyId::parse("recipient-contact-v1")
                        .map_err(BootError::Internal)?,
                    zeroize::Zeroizing::new(vec![0x31; 32]),
                )
                .map_err(BootError::Internal)?,
            ),
            resource_authorization_decisions: identity.clone(),
            privileged_authorization_decisions: identity,
            projects: projects.clone(),
            environments: projects,
            ontologies: Arc::new(InMemoryOntologyRepository::new()),
            workflow_definitions: Arc::new(InMemoryWorkflowDefinitionRepository::new()),
            workflow_goals: Arc::new(InMemoryWorkflowGoalRepository::new()),
            workflow_runs: Arc::new(InMemoryWorkflowRunRepository::new()),
            human_tasks: human_tasks
                .unwrap_or_else(|| Arc::new(TestHumanTaskRepository::default())),
            workflow_run_diagnostics: Arc::new(WorkflowRunDiagnosticsReader::new(
                FlowEngine::in_memory(Arc::new(WorkflowRunFlowRuntime::default())),
            )),
            workflow_run_history: Arc::new(EmptyWorkflowRunHistoryReader),
            workflow_run_variables: Arc::new(InputWorkflowRunVariableReader),
            forms,
            form_semantic_core,
            human_task_forms,
            search,
            audit_records: audit_records
                .unwrap_or_else(|| Arc::new(InMemoryAuditRecordRepository::new())),
            audit_export_signer: Arc::new(TestAuditExportSigner::new()),
            security_investigations: security_investigations
                .unwrap_or_else(|| Arc::new(InMemoryGatewayRoutePolicyTimelineRepository::new())),
            notifications,
            alert_policies,
            outbound_notifications,
            connector_profiles,
            connector_attempts: connector_execution.clone(),
            connector_attempt_resolutions: connector_execution.clone(),
            connector_revocations: connector_execution,
            applications: Arc::new(
                crate::modules::applications::InMemoryApplicationRepository::new(),
            ),
            application_sessions: Arc::new(
                crate::modules::applications::InMemoryApplicationSessionRepository::new(),
            ),
            developer_workflow_build_plans: Arc::new(InMemoryBuildPlanRepository::new()),
            developer_workload_profiles: Arc::new(InMemoryWorkloadProfileRepository::new()),
            developer_preview_policies: Arc::new(InMemoryPullRequestPreviewPolicyRepository::new()),
            developer_preview_projections: Arc::new(
                InMemoryPullRequestPreviewProjectionRepository::new(),
            ),
            durable_cell_applications: Arc::new(
                crate::modules::durable_cells::InMemoryDurableCellApplicationRepository::new(),
            ),
            durable_cell_deployments: Arc::new(
                crate::modules::durable_cells::InMemoryDurableCellDeploymentRepository::new(),
            ),
            oci_artifacts: Arc::new(TestOciArtifactResolver),
            plugin_registries: Arc::new(InMemoryPluginRegistryRepository::new()),
            plugin_enrollment_authorizer: Arc::new(TestPluginRegistryEnrollmentAuthorizer),
            assets: unavailable_assets,
            workloads: workload_port,
            builds,
            executions: executions.unwrap_or_else(|| Arc::new(InMemoryExecutionRepository::new())),
            execution_templates: Arc::new(InMemoryExecutionTemplateRepository::new()),
            agents: test_agents.unwrap_or_else(|| Arc::new(InMemoryAgentRepository::new())),
            agent_checkpoint_objects: Arc::new(
                agent_checkpoint_support::TestAgentExecutionCheckpointObjectStore::default(),
            ),
            agent_execution_providers: Arc::new(
                BuiltInAgentExecutionProviderRegistry::new().map_err(BootError::Internal)?,
            ),
            routes,
            mcp_credentials,
            secrets,
            user_files: user_files
                .unwrap_or_else(|| Arc::new(InMemoryUserFileRepository::default())),
            user_file_objects: user_file_objects
                .unwrap_or_else(|| Arc::new(UnavailableUserFileObjectStore)),
            sources,
            source_webhooks,
            source_subscriptions,
            github_connections,
            secret_encryption: Arc::new(TestSecretEncryption),
            route_targets,
            route_commands,
            mcp_gateway_snapshots: None,
            gateway_node_desired_state_planner: None,
            operations: operations.unwrap_or_else(|| Arc::new(InMemoryOperationRepository::new())),
            nodes: nodes.clone(),
            node_pools: nodes.clone(),
            node_control,
            log_chunks: Arc::new(TestLogChunkStore),
            readiness: HealthModule::new("readiness")
                .with_route("/health/ready")
                .indicator("repositories", || async { Ok(HealthIndicatorResult::up()) }),
        },
    )
}

fn response_json(response: &BootResponse) -> Result<Value> {
    response.body_json()
}

fn assert_no_store(response: &BootResponse) {
    assert_eq!(response.header("cache-control"), Some("no-store"));
    assert_eq!(response.header("pragma"), Some("no-cache"));
    assert_eq!(response.header("referrer-policy"), Some("no-referrer"));
}

fn response_id(response: &BootResponse) -> Result<String> {
    response_json(response)?["data"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| BootError::Internal("response does not contain a resource ID".into()))
}

async fn create_organization(
    app: &BootApplication,
    idempotency_key: &str,
    name: &str,
) -> Result<String> {
    let response = app
        .call(post_json(
            "/api/v1/organizations",
            idempotency_key,
            json!({"name": name}),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    response_id(&response)
}

async fn bootstrap_organization(
    app: &BootApplication,
    idempotency_key: &str,
    name: &str,
) -> Result<String> {
    let response = app
        .call(
            post_json(
                "/api/v1/bootstrap",
                idempotency_key,
                json!({
                    "organizationName": name,
                    "tokenName": "bootstrap-admin",
                    "token": ADMIN_TOKEN,
                    "expiresAt": null
                }),
            )
            .with_header("x-a3s-bootstrap-token", BOOTSTRAP_TOKEN),
        )
        .await?;
    assert_eq!(response.status(), 201);
    response_json(&response)?["data"]["organization"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| BootError::Internal("bootstrap response has no organization ID".into()))
}

async fn create_project(
    app: &BootApplication,
    organization_id: &str,
    idempotency_key: &str,
    name: &str,
) -> Result<String> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization_id}/projects"),
            idempotency_key,
            json!({"name": name}),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    response_id(&response)
}

async fn create_api_token(
    app: &BootApplication,
    organization_id: &str,
    idempotency_key: &str,
    name: &str,
    secret: &str,
    scopes: &[&str],
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<String> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization_id}/api-tokens"),
            idempotency_key,
            json!({
                "name": name,
                "token": secret,
                "scopes": scopes,
                "expiresAt": expires_at,
            }),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    assert!(!String::from_utf8_lossy(response.body()).contains(secret));
    response_id(&response)
}

fn runtime_capabilities() -> Value {
    json!({
        "schema": "a3s.runtime.capabilities.v4",
        "provider_id": "a3s-box",
        "provider_build": "a3s-box-test",
        "unit_classes": ["task", "service"],
        "artifact_media_types": ["application/vnd.oci.image.manifest.v1+json"],
        "isolation_levels": ["sandbox"],
        "network_modes": ["none", "service"],
        "mount_kinds": [],
        "health_check_kinds": [],
        "resource_controls": ["cpu", "memory", "pids", "ephemeral_storage"],
        "features": ["durable_identity", "stop", "remove", "service_tcp"]
    })
}

#[tokio::test]
async fn organization_writes_are_idempotent_unique_and_atomic() -> Result<()> {
    let repository = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(repository.clone(), projects)?;
    bootstrap_organization(&app, "bootstrap-root", "Root").await?;
    let request = || {
        post_json(
            "/api/v1/organizations",
            "create-acme",
            json!({"name": "Acme"}),
        )
    };

    let first = app.call(request()).await?;
    let second = app.call(request()).await?;
    let first_body = response_json(&first)?;
    let second_body = response_json(&second)?;
    assert_eq!(first.status(), 201);
    assert_eq!(second.status(), 200);
    assert_eq!(first_body["data"]["id"], second_body["data"]["id"]);
    assert_eq!(second_body["data"]["replayed"], true);

    let changed = app
        .call(post_json(
            "/api/v1/organizations",
            "create-acme",
            json!({"name": "Other"}),
        ))
        .await?;
    assert_eq!(changed.status(), 409);
    assert_eq!(response_json(&changed)?["statusCode"], "CONFLICT");

    let duplicate = app
        .call(post_json(
            "/api/v1/organizations",
            "duplicate-acme",
            json!({"name": "acme"}),
        ))
        .await?;
    assert_eq!(duplicate.status(), 409);
    let events = repository.outbox_events().await;
    assert_eq!(events.len(), 6);
    for (event_key, expected) in [
        ("identity.organization.created", 2),
        ("identity.principal.created", 1),
        ("identity.membership.created", 2),
        ("identity.token.created", 1),
    ] {
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_key == event_key)
                .count(),
            expected,
            "unexpected {event_key} event count"
        );
    }
    Ok(())
}

#[tokio::test]
async fn project_writes_are_idempotent_and_names_are_organization_scoped() -> Result<()> {
    let organizations = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(organizations, projects.clone())?;
    let acme = bootstrap_organization(&app, "organization-acme", "Acme").await?;
    let beta = create_organization(&app, "organization-beta", "Beta").await?;
    let path = format!("/api/v1/organizations/{acme}/projects");
    let request = || post_json(&path, "project-cloud", json!({"name": "Cloud"}));

    let first = app.call(request()).await?;
    let second = app.call(request()).await?;
    assert_eq!(first.status(), 201);
    assert_eq!(second.status(), 200);
    assert_eq!(response_id(&first)?, response_id(&second)?);
    assert_eq!(response_json(&second)?["data"]["replayed"], true);

    let changed = app
        .call(post_json(&path, "project-cloud", json!({"name": "Other"})))
        .await?;
    assert_eq!(changed.status(), 409);

    let duplicate = app
        .call(post_json(
            &path,
            "project-cloud-duplicate",
            json!({"name": "cloud"}),
        ))
        .await?;
    assert_eq!(duplicate.status(), 409);

    let other_scope = app
        .call(post_json(
            format!("/api/v1/organizations/{beta}/projects"),
            "project-cloud",
            json!({"name": "Cloud"}),
        ))
        .await?;
    assert_eq!(other_scope.status(), 201);
    let events = projects.outbox_events().await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_key == "project.project.created")
            .count(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn environment_writes_are_idempotent_and_names_are_project_scoped() -> Result<()> {
    let organizations = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(organizations, projects.clone())?;
    let organization = bootstrap_organization(&app, "organization", "Acme").await?;
    let cloud = create_project(&app, &organization, "project-cloud", "Cloud").await?;
    let data = create_project(&app, &organization, "project-data", "Data").await?;
    let path = format!("/api/v1/organizations/{organization}/projects/{cloud}/environments");
    let request = || {
        post_json(
            &path,
            "environment-production",
            json!({"name": "Production"}),
        )
    };

    let first = app.call(request()).await?;
    let second = app.call(request()).await?;
    assert_eq!(first.status(), 201);
    assert_eq!(second.status(), 200);
    assert_eq!(response_id(&first)?, response_id(&second)?);
    assert_eq!(response_json(&second)?["data"]["replayed"], true);

    let changed = app
        .call(post_json(
            &path,
            "environment-production",
            json!({"name": "Staging"}),
        ))
        .await?;
    assert_eq!(changed.status(), 409);

    let duplicate = app
        .call(post_json(
            &path,
            "environment-production-duplicate",
            json!({"name": "production"}),
        ))
        .await?;
    assert_eq!(duplicate.status(), 409);

    let other_scope = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{data}/environments"),
            "environment-production",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(other_scope.status(), 201);
    let events = projects.outbox_events().await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_key == "project.environment.created")
            .count(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn projects_and_environments_reject_cross_tenant_references() -> Result<()> {
    let organizations = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(organizations, projects.clone())?;
    let organization_id = bootstrap_organization(&app, "organization", "Acme").await?;
    let project_id = create_project(&app, &organization_id, "project", "Cloud").await?;

    let wrong_organization = Uuid::new_v4();
    let rejected = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{wrong_organization}/projects/{project_id}/environments"
            ),
            "wrong-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    let rejected_body = response_json(&rejected)?;
    assert_eq!(rejected.status(), 404);
    assert_eq!(rejected_body["statusCode"], "NOT_FOUND");

    let environment = app
        .call(post_json(
            format!("/api/v1/organizations/{organization_id}/projects/{project_id}/environments"),
            "environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    assert_eq!(projects.outbox_events().await.len(), 2);
    Ok(())
}

#[tokio::test]
async fn bearer_tokens_are_scoped_to_one_organization_and_never_echoed() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let acme = bootstrap_organization(&app, "bootstrap-acme", "Acme").await?;
    let beta = create_organization(&app, "organization-beta", "Beta").await?;
    create_api_token(
        &app,
        &acme,
        "token-projects",
        "project-automation",
        PROJECT_TOKEN,
        &[ApiTokenScope::PROJECT_WRITE],
        None,
    )
    .await?;

    let no_credentials = app
        .call(
            BootRequest::new(
                HttpMethod::Post,
                format!("/api/v1/organizations/{acme}/projects"),
            )
            .with_header("content-type", "application/json")
            .with_header("idempotency-key", "unauthenticated")
            .with_body(json!({"name": "Rejected"}).to_string().into_bytes()),
        )
        .await?;
    assert_eq!(no_credentials.status(), 401);

    let own_project = app
        .call(post_json_as(
            format!("/api/v1/organizations/{acme}/projects"),
            "project-own",
            json!({"name": "Own"}),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(own_project.status(), 201);

    let cross_tenant = app
        .call(post_json_as(
            format!("/api/v1/organizations/{beta}/projects"),
            "project-cross-tenant",
            json!({"name": "Rejected"}),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(cross_tenant.status(), 403);

    let scope_escalation = app
        .call(post_json_as(
            format!("/api/v1/organizations/{acme}/api-tokens"),
            "scope-escalation",
            json!({
                "name": "Escalated",
                "token": EXPIRING_TOKEN,
                "scopes": [ApiTokenScope::TOKEN_WRITE],
                "expiresAt": null
            }),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(scope_escalation.status(), 403);
    Ok(())
}

#[tokio::test]
async fn revoked_and_expired_tokens_stop_authenticating_immediately() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "bootstrap", "Acme").await?;
    let project_token_id = create_api_token(
        &app,
        &organization,
        "token-revoked",
        "revoked-token",
        PROJECT_TOKEN,
        &[ApiTokenScope::PROJECT_WRITE],
        None,
    )
    .await?;
    let revoke_path = format!("/api/v1/organizations/{organization}/api-tokens/{project_token_id}");
    let revoked = app
        .call(delete_as(&revoke_path, "revoke-project-token", ADMIN_TOKEN))
        .await?;
    assert_eq!(revoked.status(), 200);
    assert!(response_json(&revoked)?["data"]["revokedAt"].is_string());
    let replayed = app
        .call(delete_as(&revoke_path, "revoke-project-token", ADMIN_TOKEN))
        .await?;
    assert_eq!(response_json(&replayed)?["data"]["replayed"], true);

    let revoked_use = app
        .call(post_json_as(
            format!("/api/v1/organizations/{organization}/projects"),
            "revoked-use",
            json!({"name": "Rejected"}),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(revoked_use.status(), 401);

    create_api_token(
        &app,
        &organization,
        "token-expiring",
        "expiring-token",
        EXPIRING_TOKEN,
        &[ApiTokenScope::PROJECT_WRITE],
        Some(chrono::Utc::now() + chrono::Duration::milliseconds(40)),
    )
    .await?;
    tokio::time::sleep(Duration::from_millis(60)).await;
    let expired_use = app
        .call(post_json_as(
            format!("/api/v1/organizations/{organization}/projects"),
            "expired-use",
            json!({"name": "Rejected"}),
            EXPIRING_TOKEN,
        ))
        .await?;
    assert_eq!(expired_use.status(), 401);
    Ok(())
}

#[tokio::test]
async fn memberships_are_idempotent_role_authorized_and_revoke_tokens_immediately() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let app = build_test_application_with_repositories(
        Arc::clone(&identity),
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::clone(&workloads),
    )?;
    let organization = bootstrap_organization(&app, "membership-bootstrap", "Acme").await?;
    let memberships_path = format!("/api/v1/organizations/{organization}/memberships");

    let initial = app.call(get_as(&memberships_path, ADMIN_TOKEN)).await?;
    assert_eq!(initial.status(), 200);
    let initial_memberships = response_json(&initial)?["data"]
        .as_array()
        .cloned()
        .ok_or_else(|| BootError::Internal("membership list is not an array".into()))?;
    assert_eq!(initial_memberships.len(), 1);
    assert_eq!(initial_memberships[0]["role"], "owner");
    let owner_membership_id = initial_memberships[0]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("owner membership has no ID".into()))?
        .to_owned();
    let owner_principal_id = initial_memberships[0]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("owner membership has no principal ID".into()))?
        .to_owned();

    let create_body = json!({"name": "release automation", "role": "member"});
    let created = app
        .call(post_json(
            &memberships_path,
            "membership:create:release-automation",
            create_body.clone(),
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let created_data = response_json(&created)?["data"].clone();
    let membership_id = created_data["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("created membership has no ID".into()))?;
    let principal_id = created_data["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("created membership has no principal ID".into()))?;
    assert_eq!(created_data["principalKind"], "service");
    assert_eq!(created_data["aggregateVersion"], 1);

    let replayed = app
        .call(post_json(
            &memberships_path,
            "membership:create:release-automation",
            json!({
                "principalKind": "service",
                "name": "release automation",
                "role": "member"
            }),
        ))
        .await?;
    assert_eq!(replayed.status(), 200);
    let replayed_data = response_json(&replayed)?["data"].clone();
    assert_eq!(replayed_data["id"], membership_id);
    assert_eq!(replayed_data["replayed"], true);

    let human_created = app
        .call(post_json(
            &memberships_path,
            "membership:create:human",
            json!({
                "principalKind": "human",
                "name": "Ada operator",
                "role": "member"
            }),
        ))
        .await?;
    assert_eq!(human_created.status(), 201);
    let human_created = response_json(&human_created)?;
    assert_eq!(human_created["data"]["principalKind"], "human");
    assert_eq!(human_created["data"]["principalName"], "Ada operator");
    assert_eq!(human_created["data"]["role"], "member");
    assert_eq!(human_created["data"]["replayed"], false);

    let token_created = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "membership:service-token",
            json!({
                "name": "release automation",
                "token": SERVICE_MEMBER_TOKEN,
                "scopes": [
                    ApiTokenScope::PROJECT_WRITE,
                    ApiTokenScope::WORKLOAD_WRITE,
                    ApiTokenScope::IDENTITY_WRITE,
                    ApiTokenScope::TOKEN_WRITE
                ],
                "principalId": principal_id,
                "expiresAt": null,
            }),
        ))
        .await?;
    assert_eq!(token_created.status(), 201);
    assert_eq!(
        response_json(&token_created)?["data"]["principalId"],
        principal_id
    );
    assert!(!String::from_utf8_lossy(token_created.body()).contains(SERVICE_MEMBER_TOKEN));

    let privilege_escalation = app
        .call(post_json_as(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "membership:privilege-escalation",
            json!({
                "name": "forged owner credential",
                "token": PRIVILEGE_ESCALATION_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ],
                "principalId": owner_principal_id,
                "expiresAt": null,
            }),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(privilege_escalation.status(), 403);

    let member_cannot_administer = app
        .call(get_as(&memberships_path, SERVICE_MEMBER_TOKEN))
        .await?;
    assert_eq!(member_cannot_administer.status(), 403);

    let role_path = format!("{memberships_path}/{membership_id}/role");
    let promoted = app
        .call(post_json(
            &role_path,
            "membership:promote-admin",
            json!({"role": "admin", "expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(promoted.status(), 200);
    let admin_privilege_escalation = app
        .call(post_json_as(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "membership:admin-privilege-escalation",
            json!({
                "name": "forged owner credential",
                "token": PRIVILEGE_ESCALATION_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ],
                "principalId": owner_principal_id,
                "expiresAt": null,
            }),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(admin_privilege_escalation.status(), 403);
    let returned_to_member = app
        .call(post_json(
            &role_path,
            "membership:return-to-member",
            json!({"role": "member", "expectedVersion": 2}),
        ))
        .await?;
    assert_eq!(returned_to_member.status(), 200);

    let own_project = app
        .call(post_json_as(
            format!("/api/v1/organizations/{organization}/projects"),
            "membership:member-project",
            json!({"name": "Member Project"}),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(own_project.status(), 201);
    let granted_project_id = response_json(&own_project)?["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("created project has no ID".into()))?
        .parse::<Uuid>()
        .map_err(|error| BootError::Internal(format!("invalid project ID: {error}")))?;
    let ungranted_project = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects"),
            "membership:ungranted-project",
            json!({"name": "Ungranted Project"}),
        ))
        .await?;
    assert_eq!(ungranted_project.status(), 201);
    let ungranted_project_id = response_id(&ungranted_project)?;

    let granted_environment = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/projects/{granted_project_id}/environments"
            ),
            "membership:granted-environment",
            json!({"name": "Granted environment"}),
        ))
        .await?;
    assert_eq!(granted_environment.status(), 201);
    let granted_environment_id = response_id(&granted_environment)?;
    let ungranted_environment = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/projects/{ungranted_project_id}/environments"
            ),
            "membership:ungranted-environment",
            json!({"name": "Ungranted environment"}),
        ))
        .await?;
    assert_eq!(ungranted_environment.status(), 201);
    let ungranted_environment_id = response_id(&ungranted_environment)?;

    let granted_workload = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/projects/{granted_project_id}/environments/{granted_environment_id}/workloads"
            ),
            "membership:granted-workload",
            json!({
                "name": "granted-api",
                "template": workload_tests::workload_template("granted", json!([]))
            }),
        ))
        .await?;
    assert_eq!(granted_workload.status(), 202);
    let granted_workload = response_json(&granted_workload)?;
    let granted_workload_id = granted_workload["data"]["workloadId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("granted workload has no ID".into()))?
        .to_owned();
    let granted_revision_id = granted_workload["data"]["revisionId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("granted workload has no revision ID".into()))?
        .to_owned();
    let granted_deployment_id = granted_workload["data"]["deploymentId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("granted workload has no deployment ID".into()))?
        .to_owned();

    let ungranted_workload = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/projects/{ungranted_project_id}/environments/{ungranted_environment_id}/workloads"
            ),
            "membership:ungranted-workload",
            json!({
                "name": "ungranted-api",
                "template": workload_tests::workload_template("ungranted", json!([]))
            }),
        ))
        .await?;
    assert_eq!(ungranted_workload.status(), 202);
    let ungranted_workload = response_json(&ungranted_workload)?;
    let ungranted_workload_id = ungranted_workload["data"]["workloadId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("ungranted workload has no ID".into()))?
        .to_owned();
    let ungranted_revision_id = ungranted_workload["data"]["revisionId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("ungranted workload has no revision ID".into()))?
        .to_owned();
    let ungranted_deployment_id = ungranted_workload["data"]["deploymentId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("ungranted workload has no deployment ID".into()))?
        .to_owned();

    let restricted = app
        .call(post_json(
            &role_path,
            "membership:restrict",
            json!({"role": "restricted", "expectedVersion": 3}),
        ))
        .await?;
    assert_eq!(restricted.status(), 200);
    assert_eq!(response_json(&restricted)?["data"]["aggregateVersion"], 4);

    let restricted_access = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/projects"),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(restricted_access.status(), 403);

    let resource_grants_path =
        format!("/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants");
    for (idempotency_key, scope) in [
        (
            "membership:grant-missing-project",
            json!({"kind": "project", "projectId": Uuid::now_v7()}),
        ),
        (
            "membership:grant-missing-environment",
            json!({
                "kind": "environment",
                "projectId": granted_project_id,
                "environmentId": Uuid::now_v7()
            }),
        ),
        (
            "membership:grant-missing-node",
            json!({"kind": "node", "nodeId": Uuid::now_v7()}),
        ),
    ] {
        let missing_target = app
            .call(post_json(
                &resource_grants_path,
                idempotency_key,
                json!({"scope": scope}),
            ))
            .await?;
        assert_eq!(missing_target.status(), 404);
    }
    let grant_body = json!({
        "scope": {
            "kind": "project",
            "projectId": granted_project_id,
        }
    });
    let granted = app
        .call(post_json(
            &resource_grants_path,
            "membership:grant-project",
            grant_body.clone(),
        ))
        .await?;
    assert_eq!(granted.status(), 201);
    let granted_json = response_json(&granted)?;
    assert_eq!(granted_json["data"]["membershipId"], membership_id);
    assert_eq!(granted_json["data"]["scope"], grant_body["scope"]);
    assert_eq!(granted_json["data"]["aggregateVersion"], 1);
    assert_eq!(granted_json["data"]["replayed"], false);
    let resource_grant_id = granted_json["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("created Resource Grant has no ID".into()))?;

    let replayed_grant = app
        .call(post_json(
            &resource_grants_path,
            "membership:grant-project",
            grant_body,
        ))
        .await?;
    assert_eq!(replayed_grant.status(), 200);
    assert_eq!(response_json(&replayed_grant)?["data"]["replayed"], true);

    let listed_grants = app.call(get_as(&resource_grants_path, ADMIN_TOKEN)).await?;
    assert_eq!(listed_grants.status(), 200);
    assert_eq!(
        response_json(&listed_grants)?["data"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let resource_grant_path =
        format!("/api/v1/organizations/{organization}/resource-grants/{resource_grant_id}");
    let found_grant = app.call(get_as(&resource_grant_path, ADMIN_TOKEN)).await?;
    assert_eq!(found_grant.status(), 200);
    assert_eq!(
        response_json(&found_grant)?["data"]["id"],
        resource_grant_id
    );

    let granted_access = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{granted_project_id}/environments"
            ),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(granted_access.status(), 200);
    let visible_projects = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/projects"),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(visible_projects.status(), 200);
    let visible_projects = response_json(&visible_projects)?["data"]
        .as_array()
        .cloned()
        .ok_or_else(|| BootError::Internal("project list response is not an array".into()))?;
    assert_eq!(visible_projects.len(), 1);
    assert_eq!(visible_projects[0]["id"], granted_project_id.to_string());

    for path in [
        format!(
            "/api/v1/organizations/{organization}/workloads/{granted_workload_id}"
        ),
        format!(
            "/api/v1/organizations/{organization}/deployments/{granted_deployment_id}"
        ),
        format!(
            "/api/v1/organizations/{organization}/workloads/{granted_workload_id}/revisions/{granted_revision_id}/logs?limit=1"
        ),
    ] {
        let visible = app.call(get_as(path, SERVICE_MEMBER_TOKEN)).await?;
        assert_eq!(visible.status(), 200);
    }

    for (denied_path, missing_path) in [
        (
            format!(
                "/api/v1/organizations/{organization}/workloads/{ungranted_workload_id}"
            ),
            format!(
                "/api/v1/organizations/{organization}/workloads/{}",
                Uuid::now_v7()
            ),
        ),
        (
            format!(
                "/api/v1/organizations/{organization}/deployments/{ungranted_deployment_id}"
            ),
            format!(
                "/api/v1/organizations/{organization}/deployments/{}",
                Uuid::now_v7()
            ),
        ),
        (
            format!(
                "/api/v1/organizations/{organization}/workloads/{ungranted_workload_id}/revisions/{ungranted_revision_id}/logs?limit=1"
            ),
            format!(
                "/api/v1/organizations/{organization}/workloads/{}/revisions/{}/logs?limit=1",
                Uuid::now_v7(),
                Uuid::now_v7()
            ),
        ),
    ] {
        assert_resource_not_found_equivalent(
            &app,
            get_as(denied_path, SERVICE_MEMBER_TOKEN),
            get_as(missing_path, SERVICE_MEMBER_TOKEN),
        )
        .await?;
    }

    let granted_stop_path =
        format!("/api/v1/organizations/{organization}/workloads/{granted_workload_id}/stop");
    let stopped = app
        .call(post_json_as(
            &granted_stop_path,
            "membership:stop-granted-workload",
            json!({}),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(stopped.status(), 202);
    let stop_replay = app
        .call(post_json_as(
            &granted_stop_path,
            "membership:stop-granted-workload",
            json!({}),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(stop_replay.status(), 200);

    let granted_cancel_path =
        format!("/api/v1/organizations/{organization}/deployments/{granted_deployment_id}");
    let cancelled = app
        .call(delete_as(
            &granted_cancel_path,
            "membership:cancel-granted-deployment",
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(cancelled.status(), 202);
    let cancel_replay = app
        .call(delete_as(
            &granted_cancel_path,
            "membership:cancel-granted-deployment",
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(cancel_replay.status(), 200);

    assert_resource_not_found_equivalent(
        &app,
        post_json_as(
            format!("/api/v1/organizations/{organization}/workloads/{ungranted_workload_id}/stop"),
            "membership:stop-ungranted-workload",
            json!({}),
            SERVICE_MEMBER_TOKEN,
        ),
        post_json_as(
            format!(
                "/api/v1/organizations/{organization}/workloads/{}/stop",
                Uuid::now_v7()
            ),
            "membership:stop-missing-workload",
            json!({}),
            SERVICE_MEMBER_TOKEN,
        ),
    )
    .await?;
    assert_resource_not_found_equivalent(
        &app,
        delete_as(
            format!("/api/v1/organizations/{organization}/deployments/{ungranted_deployment_id}"),
            "membership:cancel-ungranted-deployment",
            SERVICE_MEMBER_TOKEN,
        ),
        delete_as(
            format!(
                "/api/v1/organizations/{organization}/deployments/{}",
                Uuid::now_v7()
            ),
            "membership:cancel-missing-deployment",
            SERVICE_MEMBER_TOKEN,
        ),
    )
    .await?;

    let update_body = json!({
        "template": workload_tests::workload_template("restricted-update", json!([]))
    });
    assert_resource_not_found_equivalent(
        &app,
        post_json_as(
            format!(
                "/api/v1/organizations/{organization}/workloads/{ungranted_workload_id}/deployments"
            ),
            "membership:update-ungranted-workload",
            update_body.clone(),
            SERVICE_MEMBER_TOKEN,
        ),
        post_json_as(
            format!(
                "/api/v1/organizations/{organization}/workloads/{}/deployments",
                Uuid::now_v7()
            ),
            "membership:update-missing-workload",
            update_body,
            SERVICE_MEMBER_TOKEN,
        ),
    )
    .await?;

    let rollback_body = json!({"revisionId": Uuid::now_v7()});
    assert_resource_not_found_equivalent(
        &app,
        post_json_as(
            format!(
                "/api/v1/organizations/{organization}/workloads/{ungranted_workload_id}/rollback"
            ),
            "membership:rollback-ungranted-workload",
            rollback_body.clone(),
            SERVICE_MEMBER_TOKEN,
        ),
        post_json_as(
            format!(
                "/api/v1/organizations/{organization}/workloads/{}/rollback",
                Uuid::now_v7()
            ),
            "membership:rollback-missing-workload",
            rollback_body,
            SERVICE_MEMBER_TOKEN,
        ),
    )
    .await?;

    let mut agent_template = workload_tests::workload_template("restricted-agent", json!([]));
    agent_template
        .as_object_mut()
        .ok_or_else(|| BootError::Internal("test workload template is not an object".into()))?
        .remove("artifact");
    let agent_update_body = json!({"template": agent_template});
    let asset_id = Uuid::now_v7();
    let release_id = Uuid::now_v7();
    assert_resource_not_found_equivalent(
        &app,
        post_json_as(
            format!(
                "/api/v1/organizations/{organization}/workloads/{ungranted_workload_id}/assets/{asset_id}/releases/{release_id}/deployments"
            ),
            "membership:agent-update-ungranted-workload",
            agent_update_body.clone(),
            SERVICE_MEMBER_TOKEN,
        ),
        post_json_as(
            format!(
                "/api/v1/organizations/{organization}/workloads/{}/assets/{asset_id}/releases/{release_id}/deployments",
                Uuid::now_v7()
            ),
            "membership:agent-update-missing-workload",
            agent_update_body,
            SERVICE_MEMBER_TOKEN,
        ),
    )
    .await?;

    let skill_id = Uuid::now_v7();
    let skill_release_id = Uuid::now_v7();
    assert_resource_not_found_equivalent(
        &app,
        post_json_as(
            format!(
                "/api/v1/organizations/{organization}/workloads/{ungranted_workload_id}/skills/{skill_id}/releases/{skill_release_id}/bindings"
            ),
            "membership:bind-ungranted-workload",
            json!({}),
            SERVICE_MEMBER_TOKEN,
        ),
        post_json_as(
            format!(
                "/api/v1/organizations/{organization}/workloads/{}/skills/{skill_id}/releases/{skill_release_id}/bindings",
                Uuid::now_v7()
            ),
            "membership:bind-missing-workload",
            json!({}),
            SERVICE_MEMBER_TOKEN,
        ),
    )
    .await?;
    assert_resource_not_found_equivalent(
        &app,
        delete_as(
            format!(
                "/api/v1/organizations/{organization}/workloads/{ungranted_workload_id}/skills/{skill_id}/bindings"
            ),
            "membership:unbind-ungranted-workload",
            SERVICE_MEMBER_TOKEN,
        ),
        delete_as(
            format!(
                "/api/v1/organizations/{organization}/workloads/{}/skills/{skill_id}/bindings",
                Uuid::now_v7()
            ),
            "membership:unbind-missing-workload",
            SERVICE_MEMBER_TOKEN,
        ),
    )
    .await?;

    let ungranted_access = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{}/environments",
                Uuid::now_v7()
            ),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(ungranted_access.status(), 403);

    let fallback_grant = app
        .call(post_json(
            &resource_grants_path,
            "membership:grant-fallback-project",
            json!({
                "scope": {
                    "kind": "project",
                    "projectId": ungranted_project_id,
                }
            }),
        ))
        .await?;
    assert_eq!(fallback_grant.status(), 201);
    let fallback_grant_id = response_json(&fallback_grant)?["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("fallback Resource Grant has no ID".into()))?
        .to_owned();

    let revoked_grant = app
        .call(post_json(
            format!("{resource_grant_path}/revocation"),
            "membership:revoke-project",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(revoked_grant.status(), 200);
    assert_eq!(
        response_json(&revoked_grant)?["data"]["aggregateVersion"],
        2
    );
    assert_resource_not_found_equivalent(
        &app,
        post_json_as(
            &granted_stop_path,
            "membership:stop-granted-workload",
            json!({}),
            SERVICE_MEMBER_TOKEN,
        ),
        post_json_as(
            format!(
                "/api/v1/organizations/{organization}/workloads/{}/stop",
                Uuid::now_v7()
            ),
            "membership:stop-missing-after-revoke",
            json!({}),
            SERVICE_MEMBER_TOKEN,
        ),
    )
    .await?;
    assert_resource_not_found_equivalent(
        &app,
        delete_as(
            &granted_cancel_path,
            "membership:cancel-granted-deployment",
            SERVICE_MEMBER_TOKEN,
        ),
        delete_as(
            format!(
                "/api/v1/organizations/{organization}/deployments/{}",
                Uuid::now_v7()
            ),
            "membership:cancel-missing-after-revoke",
            SERVICE_MEMBER_TOKEN,
        ),
    )
    .await?;

    let fallback_revoked = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/resource-grants/{fallback_grant_id}/revocation"
            ),
            "membership:revoke-fallback-project",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(fallback_revoked.status(), 200);
    let revoked_access = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{granted_project_id}/environments"
            ),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(revoked_access.status(), 403);
    let revoked_collection_access = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/projects"),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(revoked_collection_access.status(), 403);

    let restored = app
        .call(post_json(
            &role_path,
            "membership:restore",
            json!({"role": "member", "expectedVersion": 4}),
        ))
        .await?;
    assert_eq!(restored.status(), 200);
    assert_eq!(response_json(&restored)?["data"]["aggregateVersion"], 5);

    let restored_access = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/projects"),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(restored_access.status(), 200);

    let revoked = app
        .call(post_json(
            format!("{memberships_path}/{membership_id}/revocation"),
            "membership:revoke",
            json!({"expectedVersion": 5}),
        ))
        .await?;
    assert_eq!(revoked.status(), 200);
    assert!(response_json(&revoked)?["data"]["revokedAt"].is_string());

    let revoked_access = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/projects"),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(revoked_access.status(), 401);

    let last_owner = app
        .call(post_json(
            format!("{memberships_path}/{owner_membership_id}/revocation"),
            "membership:last-owner",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(last_owner.status(), 409);
    Ok(())
}

#[tokio::test]
async fn membership_invitations_bind_exact_principals_and_accept_atomically() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let app = build_test_application(
        Arc::clone(&identity),
        Arc::new(InMemoryProjectsRepository::new()),
    )?;
    let source_organization =
        bootstrap_organization(&app, "invitation-bootstrap", "Source").await?;
    let target_organization = create_organization(&app, "invitation-target", "Target").await?;

    let service = app
        .call(post_json(
            format!("/api/v1/organizations/{source_organization}/memberships"),
            "invitation:create-principal",
            json!({"name": "invited automation", "role": "member"}),
        ))
        .await?;
    assert_eq!(service.status(), 201);
    let service = response_json(&service)?;
    let principal_id = service["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("invited service has no Principal ID".into()))?;

    let credential = app
        .call(post_json(
            format!("/api/v1/organizations/{source_organization}/api-tokens"),
            "invitation:create-credential",
            json!({
                "name": "invitation acceptance",
                "token": SERVICE_MEMBER_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ, ApiTokenScope::IDENTITY_WRITE],
                "principalId": principal_id,
                "expiresAt": null,
            }),
        ))
        .await?;
    assert_eq!(credential.status(), 201);

    let invitation_path =
        format!("/api/v1/organizations/{target_organization}/membership-invitations");
    let invitation_body = json!({
        "principalId": principal_id,
        "role": "restricted",
        "expiresAt": Utc::now() + chrono::Duration::days(7),
    });
    let created = app
        .call(post_json(
            &invitation_path,
            "invitation:create",
            invitation_body.clone(),
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let created = response_json(&created)?;
    let invitation_id = created["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("membership invitation has no ID".into()))?;
    assert_eq!(created["data"]["status"], "pending");
    assert_eq!(created["data"]["aggregateVersion"], 1);

    let replayed = app
        .call(post_json(
            &invitation_path,
            "invitation:create",
            invitation_body,
        ))
        .await?;
    assert_eq!(replayed.status(), 200);
    assert_eq!(response_json(&replayed)?["data"]["replayed"], true);

    let mine = app
        .call(get_as(
            "/api/v1/membership-invitations",
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(mine.status(), 200);
    let mine = response_json(&mine)?;
    assert_eq!(mine["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(mine["data"][0]["id"], invitation_id);

    let acceptance_path = format!("/api/v1/membership-invitations/{invitation_id}/acceptance");
    let guessed = app
        .call(post_json(
            &acceptance_path,
            "invitation:wrong-principal",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(guessed.status(), 404);

    let accepted = app
        .call(post_json_as(
            &acceptance_path,
            "invitation:accept",
            json!({"expectedVersion": 1}),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(accepted.status(), 201);
    let accepted = response_json(&accepted)?;
    assert_eq!(accepted["data"]["invitation"]["status"], "accepted");
    assert_eq!(accepted["data"]["invitation"]["aggregateVersion"], 2);
    assert_eq!(accepted["data"]["membership"]["principalId"], principal_id);
    assert_eq!(accepted["data"]["membership"]["role"], "restricted");

    let accepted_replay = app
        .call(post_json_as(
            &acceptance_path,
            "invitation:accept",
            json!({"expectedVersion": 1}),
            SERVICE_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(accepted_replay.status(), 200);
    assert_eq!(response_json(&accepted_replay)?["data"]["replayed"], true);

    let duplicate = app
        .call(post_json(
            &invitation_path,
            "invitation:duplicate-membership",
            json!({
                "principalId": principal_id,
                "role": "member",
                "expiresAt": Utc::now() + chrono::Duration::days(7),
            }),
        ))
        .await?;
    assert_eq!(duplicate.status(), 409);

    let invitation_events = identity
        .outbox_events()
        .await
        .into_iter()
        .filter(|event| {
            event
                .event_key
                .starts_with("identity.membership-invitation.")
        })
        .collect::<Vec<_>>();
    assert_eq!(invitation_events.len(), 2);
    assert_eq!(
        invitation_events[0].event_key,
        "identity.membership-invitation.created"
    );
    assert_eq!(
        invitation_events[1].event_key,
        "identity.membership-invitation.accepted"
    );
    Ok(())
}

#[tokio::test]
async fn api_token_queries_are_tenant_scoped_and_never_expose_credentials() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "bootstrap-token-query", "Acme").await?;
    let other_organization = create_organization(&app, "token-query-other", "Other").await?;
    create_api_token(
        &app,
        &organization,
        "token-manager",
        "token-manager",
        TOKEN_MANAGER_TOKEN,
        &[ApiTokenScope::TOKEN_WRITE],
        None,
    )
    .await?;
    let project_token_id = create_api_token(
        &app,
        &organization,
        "token-query-project",
        "project-automation",
        PROJECT_TOKEN,
        &[ApiTokenScope::PROJECT_WRITE],
        None,
    )
    .await?;
    let collection_path = format!("/api/v1/organizations/{organization}/api-tokens");

    let listed = app
        .call(get_as(&collection_path, TOKEN_MANAGER_TOKEN))
        .await?;
    assert_eq!(listed.status(), 200);
    let listed_body = String::from_utf8_lossy(listed.body());
    assert!(!listed_body.contains(ADMIN_TOKEN));
    assert!(!listed_body.contains(TOKEN_MANAGER_TOKEN));
    assert!(!listed_body.contains(PROJECT_TOKEN));
    let listed_json = response_json(&listed)?;
    let tokens = listed_json["data"]
        .as_array()
        .ok_or_else(|| BootError::Internal("API token list response is not an array".into()))?;
    assert_eq!(tokens.len(), 3);
    assert!(tokens.iter().any(|token| token["id"] == project_token_id));
    assert!(tokens.iter().all(|token| token.get("replayed").is_none()));

    let detail = app
        .call(get_as(
            format!("{collection_path}/{project_token_id}"),
            TOKEN_MANAGER_TOKEN,
        ))
        .await?;
    assert_eq!(detail.status(), 200);
    assert_eq!(response_json(&detail)?["data"]["id"], project_token_id);
    assert!(!String::from_utf8_lossy(detail.body()).contains(PROJECT_TOKEN));

    let unknown = app
        .call(get_as(
            format!("{collection_path}/{}", Uuid::now_v7()),
            TOKEN_MANAGER_TOKEN,
        ))
        .await?;
    assert_eq!(unknown.status(), 404);

    let insufficient_scope = app.call(get_as(&collection_path, PROJECT_TOKEN)).await?;
    assert_eq!(insufficient_scope.status(), 403);

    let cross_tenant = app
        .call(get_as(
            format!("/api/v1/organizations/{other_organization}/api-tokens"),
            TOKEN_MANAGER_TOKEN,
        ))
        .await?;
    assert_eq!(cross_tenant.status(), 403);

    let unauthenticated = app
        .call(BootRequest::new(HttpMethod::Get, collection_path))
        .await?;
    assert_eq!(unauthenticated.status(), 401);
    Ok(())
}

#[tokio::test]
async fn fleet_api_enrolls_lists_and_changes_node_state_without_exposing_secrets() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "fleet-bootstrap", "Acme").await?;
    create_api_token(
        &app,
        &organization,
        "fleet-limited-token",
        "project-only",
        PROJECT_TOKEN,
        &[ApiTokenScope::PROJECT_WRITE],
        None,
    )
    .await?;

    let enrollment_secret = format!("a3sn_{}", "d".repeat(64));
    let token_path = format!("/api/v1/organizations/{organization}/enrollment-tokens");
    let forbidden = app
        .call(post_json_as(
            &token_path,
            "fleet-token-forbidden",
            json!({
                "name": "worker",
                "token": enrollment_secret,
                "expiresAt": Utc::now() + chrono::Duration::minutes(10)
            }),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(forbidden.status(), 403);
    let issued = app
        .call(post_json(
            &token_path,
            "fleet-token",
            json!({
                "name": "worker",
                "token": enrollment_secret,
                "expiresAt": Utc::now() + chrono::Duration::minutes(10)
            }),
        ))
        .await?;
    assert_eq!(issued.status(), 201);
    assert!(!String::from_utf8_lossy(issued.body()).contains(&enrollment_secret));

    let agent_instance_id = Uuid::now_v7();
    let enrolled = app
            .call(
                BootRequest::new(HttpMethod::Post, "/api/v1/node-control/enroll")
                    .with_header("content-type", "application/json")
                    .with_body(
                        json!({
                            "schema": "a3s.cloud.node-enrollment-request.v1",
                            "enrollment_token": enrollment_secret,
                            "node_name": "worker-1",
                            "agent_instance_id": agent_instance_id,
                            "agent_version": "0.1.0",
                            "csr_pem": "-----BEGIN CERTIFICATE REQUEST-----\ndGVzdA==\n-----END CERTIFICATE REQUEST-----\n",
                            "runtime_capabilities": runtime_capabilities()
                        })
                        .to_string()
                        .into_bytes(),
                    ),
            )
            .await?;
    assert_eq!(enrolled.status(), 201);
    let enrollment = response_json(&enrolled)?;
    assert_eq!(
        enrollment["schema"],
        "a3s.cloud.node-enrollment-response.v1"
    );
    let node_id = enrollment["node_id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("enrollment response has no node ID".into()))?;

    let nodes_path = format!("/api/v1/organizations/{organization}/nodes");
    let listed = app.call(get_as(&nodes_path, ADMIN_TOKEN)).await?;
    assert_eq!(listed.status(), 200);
    assert_eq!(response_json(&listed)?["data"][0]["state"], "pending");
    let node_path = format!("{nodes_path}/{node_id}");
    let found = app.call(get_as(&node_path, ADMIN_TOKEN)).await?;
    assert_eq!(response_json(&found)?["data"]["name"], "worker-1");

    let drain_path = format!("{node_path}/actions/drain");
    let drained = app
        .call(post_json(
            &drain_path,
            "fleet-drain",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(drained.status(), 200);
    assert_eq!(response_json(&drained)?["data"]["state"], "draining");
    let drain_replay = app
        .call(post_json(
            &drain_path,
            "fleet-drain",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(response_json(&drain_replay)?["data"]["replayed"], true);
    let revoked = app
        .call(post_json(
            format!("{node_path}/actions/revoke"),
            "fleet-revoke",
            json!({"expectedVersion": 2}),
        ))
        .await?;
    assert_eq!(revoked.status(), 200);
    assert_eq!(response_json(&revoked)?["data"]["state"], "revoked");
    Ok(())
}

#[tokio::test]
async fn authenticated_queries_and_operation_stream_return_authoritative_snapshots() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "bootstrap", "Acme").await?;
    let project = create_project(&app, &organization, "project", "Cloud").await?;
    let environment_path =
        format!("/api/v1/organizations/{organization}/projects/{project}/environments");
    let environment = app
        .call(post_json(
            &environment_path,
            "environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);

    let organizations = app
        .call(get_as("/api/v1/organizations", ADMIN_TOKEN))
        .await?;
    assert_eq!(response_json(&organizations)?["data"][0]["name"], "Acme");
    let listed_projects = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/projects"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(response_json(&listed_projects)?["data"][0]["name"], "Cloud");
    let environments = app.call(get_as(&environment_path, ADMIN_TOKEN)).await?;
    assert_eq!(
        response_json(&environments)?["data"][0]["name"],
        "Production"
    );
    let operations = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/operations"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(response_json(&operations)?["data"], json!([]));

    let stream = app
        .call(
            BootRequest::new(
                HttpMethod::Get,
                format!("/api/v1/organizations/{organization}/operations/stream"),
            )
            .with_header("accept", "text/event-stream")
            .with_header("authorization", format!("Bearer {ADMIN_TOKEN}")),
        )
        .await?;
    assert_eq!(stream.status(), 200);
    assert!(stream.is_streaming());
    assert!(stream.is_event_stream());
    Ok(())
}
