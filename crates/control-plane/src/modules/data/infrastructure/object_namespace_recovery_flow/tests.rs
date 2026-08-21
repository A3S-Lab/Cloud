use super::*;
use crate::infrastructure::{ImmutableObjectClient, BOUNDED_STEP_RETRY_PATCH_ID};
use crate::modules::data::application::{
    DeleteObjectNamespaceOperationInput, DeleteObjectNamespaceOperationOutput,
    ObjectNamespaceAccess, ObjectNamespaceFlowBinding, ObjectNamespaceRecoveryOperationRequest,
    ObjectNamespaceRecoveryStore, RestoreObjectNamespaceOperationInput,
    RestoreObjectNamespaceOperationOutput, SealObjectNamespaceOperationInput,
    SealObjectNamespaceOperationOutput, OBJECT_NAMESPACE_DELETE_WORKFLOW_NAME,
    OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION, OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME,
    OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME,
};
use crate::modules::data::domain::{
    IObjectNamespace, ObjectNamespaceCredentialBinding, ObjectNamespaceCredentialBindingSpec,
    ObjectNamespaceDeletionPlan, ObjectNamespaceError, ObjectNamespaceKey,
    ObjectNamespaceProviderProfile, ObjectNamespaceProviderProfileSpec, ObjectNamespaceRestorePlan,
    ObjectNamespaceRetentionPolicy, ObjectNamespaceRetentionPolicySpec,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, OperationId, OrganizationId, ProjectId, SecretId,
    SecretVersionReference, Sha256Digest, StorageNamespaceId,
};
use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, InMemoryEventStore,
    WorkflowPatchId, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use object_store::memory::InMemory;
use object_store::ObjectStore;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

mod process_death;

#[derive(Default)]
struct InMemoryAccessResolver {
    live: Mutex<BTreeMap<StorageNamespaceId, Arc<dyn IObjectNamespace>>>,
    recovery: Mutex<BTreeMap<StorageNamespaceId, Arc<dyn IObjectNamespace>>>,
}

impl InMemoryAccessResolver {
    fn register_live(
        &self,
        namespace_id: StorageNamespaceId,
        namespace: Arc<dyn IObjectNamespace>,
    ) {
        self.live
            .lock()
            .expect("live namespace lock")
            .insert(namespace_id, namespace);
    }

    fn register_recovery(
        &self,
        namespace_id: StorageNamespaceId,
        namespace: Arc<dyn IObjectNamespace>,
    ) {
        self.recovery
            .lock()
            .expect("recovery namespace lock")
            .insert(namespace_id, namespace);
    }

    fn lookup(
        namespaces: &Mutex<BTreeMap<StorageNamespaceId, Arc<dyn IObjectNamespace>>>,
        namespace_id: StorageNamespaceId,
        label: &str,
    ) -> Result<Arc<dyn IObjectNamespace>, ObjectNamespaceError> {
        namespaces
            .lock()
            .map_err(|_| ObjectNamespaceError::Unavailable(format!("{label} lock poisoned")))?
            .get(&namespace_id)
            .cloned()
            .ok_or_else(|| {
                ObjectNamespaceError::Invalid(format!("{label} was not registered for the test"))
            })
    }
}

#[async_trait]
impl IObjectNamespaceAccessResolver for InMemoryAccessResolver {
    async fn source_and_recovery(
        &self,
        binding: &ObjectNamespaceFlowBinding,
    ) -> Result<(ObjectNamespaceAccess, ObjectNamespaceRecoveryStore), ObjectNamespaceError> {
        let namespace_id = binding.credentials.spec().namespace_id;
        let profile = binding.provider_profile.digest().clone();
        let live = Self::lookup(&self.live, namespace_id, "live namespace")?;
        let recovery = Self::lookup(&self.recovery, namespace_id, "recovery namespace")?;
        Ok((
            ObjectNamespaceAccess::new(namespace_id, profile.clone(), live)
                .map_err(ObjectNamespaceError::Invalid)?,
            ObjectNamespaceRecoveryStore::new(profile, recovery)
                .map_err(ObjectNamespaceError::Invalid)?,
        ))
    }

    async fn access(
        &self,
        binding: &ObjectNamespaceFlowBinding,
    ) -> Result<ObjectNamespaceAccess, ObjectNamespaceError> {
        let namespace_id = binding.credentials.spec().namespace_id;
        ObjectNamespaceAccess::new(
            namespace_id,
            binding.provider_profile.digest().clone(),
            Self::lookup(&self.live, namespace_id, "live namespace")?,
        )
        .map_err(ObjectNamespaceError::Invalid)
    }
}

#[tokio::test]
async fn operations_flow_seals_restores_waits_and_deletes_without_another_lifecycle(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    for index in 0..33 {
        put(
            &fixture.source_namespace,
            &format!("state/{index:04}.bin"),
            format!("value-{index:04}").as_bytes(),
        )
        .await?;
    }
    let engine = FlowEngine::in_memory(Arc::new(fixture.runtime()));
    let sealed_at = canonical_timestamp(Utc::now());

    let seal_operation_id = OperationId::new();
    let seal_request =
        ObjectNamespaceRecoveryOperationRequest::seal(SealObjectNamespaceOperationInput {
            operation_id: seal_operation_id,
            organization_id: fixture.organization_id,
            source: fixture.source_binding.clone(),
            previous_recovery_point: None,
            writer_epoch: 7,
            writer_fence_receipt_digest: digest('a'),
            sealed_at,
        })?;
    start(&engine, &seal_request).await?;
    let sealed: SealObjectNamespaceOperationOutput = output(&engine, seal_operation_id).await?;
    assert_eq!(sealed.recovery_point.spec().writer_epoch, 7);
    assert_eq!(
        created_step_ids(&engine, seal_operation_id).await?,
        vec![
            "seal-snapshot-0000",
            "seal-snapshot-0001",
            "seal-verify-0000",
            "seal-verify-0001",
            "seal-finalize",
        ],
        "a namespace-sized seal must persist one bounded checkpoint per page"
    );

    let retention_policy = retention_policy()?;
    let restore_plan = ObjectNamespaceRestorePlan::for_recovery_point(
        &sealed.recovery_point,
        fixture.target_namespace_id,
        fixture.profile.digest().clone(),
        &retention_policy,
        sealed_at + Duration::seconds(1),
    )?;
    let restore_operation_id = OperationId::new();
    let restore_request =
        ObjectNamespaceRecoveryOperationRequest::restore(RestoreObjectNamespaceOperationInput {
            operation_id: restore_operation_id,
            organization_id: fixture.organization_id,
            source: fixture.source_binding.clone(),
            target: fixture.target_binding.clone(),
            recovery_point: sealed.recovery_point.clone(),
            restore_plan: restore_plan.clone(),
            retention_policy: retention_policy.clone(),
        })?;
    start(&engine, &restore_request).await?;
    let restored: RestoreObjectNamespaceOperationOutput =
        output(&engine, restore_operation_id).await?;
    restored
        .restore_evidence
        .validate_for(&restore_plan)
        .expect("exact restore evidence");
    assert_eq!(
        fixture.target_namespace.list(None, 64, 8192).await?.len(),
        33
    );
    assert_eq!(
        created_step_ids(&engine, restore_operation_id).await?,
        vec![
            "restore-preflight-0000",
            "restore-apply-0000",
            "restore-apply-0001",
            "restore-verify-0000",
            "restore-verify-0001",
            "restore-finalize",
        ],
        "restore must preflight, apply, and verify bounded pages"
    );

    let deletion_requested_at = restored.restore_evidence.verified_at + Duration::seconds(1);
    let deletion_plan = ObjectNamespaceDeletionPlan::after_verified_restore(
        &sealed.recovery_point,
        &restore_plan,
        &restored.restore_evidence,
        &retention_policy,
        digest('b'),
        digest('c'),
        deletion_requested_at,
    )?;
    let delete_operation_id = OperationId::new();
    let delete_request =
        ObjectNamespaceRecoveryOperationRequest::delete(DeleteObjectNamespaceOperationInput {
            operation_id: delete_operation_id,
            organization_id: fixture.organization_id,
            source: fixture.source_binding.clone(),
            retained_restore: fixture.target_binding.clone(),
            recovery_point: sealed.recovery_point,
            restore_plan,
            restore_evidence: restored.restore_evidence,
            deletion_plan: deletion_plan.clone(),
            retention_policy,
        })?;
    start(&engine, &delete_request).await?;
    assert_eq!(
        engine
            .snapshot(&delete_operation_id.to_string())
            .await?
            .status,
        WorkflowRunStatus::Suspended
    );
    assert_eq!(
        fixture.source_namespace.list(None, 64, 8192).await?.len(),
        33,
        "Flow must not execute deletion before its durable grace wait"
    );

    engine
        .resume_due_waits(deletion_plan.spec().not_before)
        .await?;
    let deleted: DeleteObjectNamespaceOperationOutput =
        output(&engine, delete_operation_id).await?;
    deleted
        .deletion_evidence
        .validate_for(&deletion_plan)
        .expect("exact deletion evidence");
    assert!(fixture
        .source_namespace
        .list(None, 64, 8192)
        .await?
        .is_empty());
    assert!(fixture
        .recovery_namespace
        .list(None, 16, 16 * 1024)
        .await?
        .is_empty());
    assert_eq!(
        fixture.target_namespace.list(None, 64, 8192).await?.len(),
        33
    );
    assert_eq!(
        created_step_ids(&engine, delete_operation_id).await?,
        vec![
            "delete-retained-preflight-0000",
            "delete-retained-preflight-0001",
            "delete-source-preflight-0000",
            "delete-source-preflight-0001",
            "delete-mark",
            "delete-source-0000",
            "delete-source-0001",
            "delete-source-absence",
            "delete-recovery-plan-0000",
            "delete-recovery-0000",
            "delete-recovery-plan-0001",
            "delete-recovery-0001",
            "delete-retained-postflight-0000",
            "delete-retained-postflight-0001",
            "delete-recovery-anchor",
            "delete-finalize",
        ],
        "delete must checkpoint exact source, recovery, and retained-restore pages"
    );
    Ok(())
}

#[tokio::test]
async fn flow_completion_loss_replays_the_exact_provider_manifest(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    put(&fixture.source_namespace, "state", b"one").await?;
    let operation_id = OperationId::new();
    let request =
        ObjectNamespaceRecoveryOperationRequest::seal(SealObjectNamespaceOperationInput {
            operation_id,
            organization_id: fixture.organization_id,
            source: fixture.source_binding.clone(),
            previous_recovery_point: None,
            writer_epoch: 1,
            writer_fence_receipt_digest: digest('d'),
            sealed_at: canonical_timestamp(Utc::now()),
        })?;
    let store = Arc::new(FailStepCompletionStore::new("seal-snapshot-0000"));
    let runtime = Arc::new(fixture.runtime());
    let engine = FlowEngine::new(store.clone(), runtime.clone());
    let failure = engine
        .start_with_id(
            operation_id.to_string(),
            workflow_spec(&request),
            request.input.clone(),
        )
        .await
        .expect_err("injected Flow completion loss");
    assert!(matches!(failure, FlowError::Store(_)));
    let before = fixture.recovery_namespace.list(None, 8, 16 * 1024).await?;
    assert_eq!(
        before.len(),
        1,
        "the lost page completion must not publish the manifest early"
    );

    drop(engine);
    let engine = FlowEngine::new(store, runtime);
    engine
        .start_with_id(
            operation_id.to_string(),
            workflow_spec(&request),
            request.input,
        )
        .await?;
    let replayed: SealObjectNamespaceOperationOutput = output(&engine, operation_id).await?;
    assert_eq!(replayed.recovery_point.spec().sequence, 1);
    let after = fixture.recovery_namespace.list(None, 8, 16 * 1024).await?;
    assert_eq!(after.len(), 2, "one exact snapshot plus the final manifest");
    assert!(
        before.iter().all(|entry| after.contains(entry)),
        "replay must adopt the exact immutable snapshot bytes"
    );
    Ok(())
}

#[tokio::test]
async fn legacy_version_one_remains_explicitly_replayable() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new()?;
    put(&fixture.source_namespace, "legacy/state", b"one").await?;
    let operation_id = OperationId::new();
    let request =
        ObjectNamespaceRecoveryOperationRequest::seal(SealObjectNamespaceOperationInput {
            operation_id,
            organization_id: fixture.organization_id,
            source: fixture.source_binding.clone(),
            previous_recovery_point: None,
            writer_epoch: 1,
            writer_fence_receipt_digest: digest('7'),
            sealed_at: canonical_timestamp(Utc::now()),
        })?;
    let engine = FlowEngine::in_memory(Arc::new(fixture.runtime()));
    engine
        .start_with_id(
            operation_id.to_string(),
            WorkflowSpec::rust_embedded(
                OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME,
                LEGACY_OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION,
                "a3s-cloud",
                "main",
            ),
            request.input,
        )
        .await?;
    let sealed: SealObjectNamespaceOperationOutput = output(&engine, operation_id).await?;
    assert_eq!(sealed.recovery_point.spec().sequence, 1);
    assert_eq!(
        created_step_ids(&engine, operation_id).await?,
        vec!["execute"]
    );
    Ok(())
}

#[test]
fn operation_requests_are_exact_non_secret_contracts_and_reject_cross_scope_restore(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let input = SealObjectNamespaceOperationInput {
        operation_id: OperationId::new(),
        organization_id: fixture.organization_id,
        source: fixture.source_binding.clone(),
        previous_recovery_point: None,
        writer_epoch: 1,
        writer_fence_receipt_digest: digest('e'),
        sealed_at: canonical_timestamp(Utc::now()),
    };
    let request = ObjectNamespaceRecoveryOperationRequest::seal(input)?;
    assert_eq!(request.workflow.name(), OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME);
    let encoded = serde_json::to_string(&request.input)?;
    for plaintext in [
        "plain-access-key",
        "plain-secret-key",
        "plain-session-token",
    ] {
        assert!(!encoded.contains(plaintext));
    }

    let point = crate::modules::data::ObjectNamespaceRecoveryPoint::seal(
        crate::modules::data::ObjectNamespaceRecoveryPointSpec {
            namespace_id: fixture.source_namespace_id,
            sequence: 1,
            writer_epoch: 1,
            provider_profile_digest: fixture.profile.digest().clone(),
            manifest_key: ObjectNamespaceKey::parse("manifests/1.json")?,
            manifest_digest: digest('f'),
            state_digest: digest('1'),
            state_size_bytes: 1,
            predecessor_digest: None,
            sealed_at: canonical_timestamp(Utc::now()),
        },
    )?;
    let policy = retention_policy()?;
    let plan = ObjectNamespaceRestorePlan::for_recovery_point(
        &point,
        fixture.target_namespace_id,
        fixture.profile.digest().clone(),
        &policy,
        point.spec().sealed_at + Duration::seconds(1),
    )?;
    let mut foreign = fixture.target_binding.clone();
    foreign.credentials =
        ObjectNamespaceCredentialBinding::from_spec(ObjectNamespaceCredentialBindingSpec {
            environment_id: EnvironmentId::new(),
            ..foreign.credentials.spec().clone()
        })?;
    assert!(ObjectNamespaceRecoveryOperationRequest::restore(
        RestoreObjectNamespaceOperationInput {
            operation_id: OperationId::new(),
            organization_id: fixture.organization_id,
            source: fixture.source_binding,
            target: foreign,
            recovery_point: point,
            restore_plan: plan,
            retention_policy: policy,
        }
    )
    .is_err());
    Ok(())
}

struct Fixture {
    organization_id: OrganizationId,
    source_namespace_id: StorageNamespaceId,
    target_namespace_id: StorageNamespaceId,
    profile: ObjectNamespaceProviderProfile,
    source_binding: ObjectNamespaceFlowBinding,
    target_binding: ObjectNamespaceFlowBinding,
    source_namespace: Arc<dyn IObjectNamespace>,
    recovery_namespace: Arc<dyn IObjectNamespace>,
    target_namespace: Arc<dyn IObjectNamespace>,
    resolver: Arc<InMemoryAccessResolver>,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let source_namespace_id = StorageNamespaceId::new();
        let target_namespace_id = StorageNamespaceId::new();
        let profile =
            ObjectNamespaceProviderProfile::from_spec(ObjectNamespaceProviderProfileSpec {
                endpoint: "https://s3.example.com".into(),
                region: "us-east-1".into(),
                bucket: "a3s-recovery-tests".into(),
                prefix: "tests/object-namespaces".into(),
                virtual_hosted_style: false,
            })?;
        let source_binding = flow_binding(
            &profile,
            organization_id,
            project_id,
            environment_id,
            source_namespace_id,
        )?;
        let target_binding = flow_binding(
            &profile,
            organization_id,
            project_id,
            environment_id,
            target_namespace_id,
        )?;
        let objects = Arc::new(InMemory::new()) as Arc<dyn ObjectStore>;
        let source_namespace = namespace(&objects, "live/source")?;
        let recovery_namespace = namespace(&objects, "recovery/source")?;
        let target_namespace = namespace(&objects, "live/target")?;
        let resolver = Arc::new(InMemoryAccessResolver::default());
        resolver.register_live(source_namespace_id, source_namespace.clone());
        resolver.register_recovery(source_namespace_id, recovery_namespace.clone());
        resolver.register_live(target_namespace_id, target_namespace.clone());
        Ok(Self {
            organization_id,
            source_namespace_id,
            target_namespace_id,
            profile,
            source_binding,
            target_binding,
            source_namespace,
            recovery_namespace,
            target_namespace,
            resolver,
        })
    }

    fn runtime(&self) -> ObjectNamespaceRecoveryFlowRuntime {
        ObjectNamespaceRecoveryFlowRuntime::with_resolver(
            self.resolver.clone(),
            ObjectNamespaceRecoveryExecutor::new(64, 1024, 8192, 8192)
                .expect("test recovery bounds"),
        )
    }
}

fn flow_binding(
    profile: &ObjectNamespaceProviderProfile,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    namespace_id: StorageNamespaceId,
) -> Result<ObjectNamespaceFlowBinding, String> {
    Ok(ObjectNamespaceFlowBinding {
        provider_profile: profile.clone(),
        credentials: ObjectNamespaceCredentialBinding::from_spec(
            ObjectNamespaceCredentialBindingSpec {
                organization_id,
                project_id,
                environment_id,
                namespace_id,
                generation: 1,
                provider_profile_digest: profile.digest().clone(),
                access_key_id: secret_reference(),
                secret_access_key: secret_reference(),
                session_token: Some(secret_reference()),
            },
        )?,
    })
}

fn secret_reference() -> SecretVersionReference {
    SecretVersionReference::new(SecretId::new(), 1).expect("Secret reference")
}

fn namespace(
    objects: &Arc<dyn ObjectStore>,
    prefix: &str,
) -> Result<Arc<dyn IObjectNamespace>, Box<dyn std::error::Error>> {
    Ok(Arc::new(ImmutableObjectClient::from_store(
        objects.clone(),
        prefix,
    )?))
}

async fn put(
    namespace: &Arc<dyn IObjectNamespace>,
    key: &str,
    body: &[u8],
) -> Result<(), ObjectNamespaceError> {
    namespace
        .conditional_create(
            &ObjectNamespaceKey::parse(key).map_err(ObjectNamespaceError::Invalid)?,
            body.to_vec(),
            1024,
        )
        .await
        .map(|_| ())
}

async fn start(
    engine: &FlowEngine,
    request: &crate::modules::operations::OperationRequest,
) -> Result<(), FlowError> {
    engine
        .start_with_id(
            request.id.to_string(),
            workflow_spec(request),
            request.input.clone(),
        )
        .await
        .map(|_| ())
}

fn workflow_spec(request: &crate::modules::operations::OperationRequest) -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        request.workflow.name(),
        request.workflow.version(),
        "a3s-cloud",
        "main",
    )
    .with_patch_marker(
        WorkflowPatchId::new(BOUNDED_STEP_RETRY_PATCH_ID)
            .expect("bounded retry patch ID must remain valid"),
    )
}

async fn created_step_ids(
    engine: &FlowEngine,
    operation_id: OperationId,
) -> Result<Vec<String>, FlowError> {
    Ok(engine
        .history(&operation_id.to_string())
        .await?
        .into_iter()
        .filter_map(|envelope| match envelope.event {
            FlowEvent::StepCreated { step_id, .. } => Some(step_id),
            _ => None,
        })
        .collect())
}

async fn output<T: serde::de::DeserializeOwned>(
    engine: &FlowEngine,
    operation_id: OperationId,
) -> Result<T, Box<dyn std::error::Error>> {
    let snapshot = engine.snapshot(&operation_id.to_string()).await?;
    if snapshot.status != WorkflowRunStatus::Completed {
        return Err(format!("operation did not complete: {:?}", snapshot.status).into());
    }
    Ok(serde_json::from_value(
        snapshot.output.ok_or("operation output missing")?,
    )?)
}

fn retention_policy() -> Result<ObjectNamespaceRetentionPolicy, String> {
    ObjectNamespaceRetentionPolicy::from_spec(ObjectNamespaceRetentionPolicySpec {
        minimum_sealed_recovery_points: 1,
        maximum_sealed_recovery_points: 4,
        maximum_recovery_point_age_seconds: 24 * 60 * 60,
        deletion_grace_period_seconds: 5 * 60,
    })
}

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
}

struct FailStepCompletionStore {
    inner: InMemoryEventStore,
    step_id: Mutex<Option<&'static str>>,
}

impl FailStepCompletionStore {
    fn new(step_id: &'static str) -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            step_id: Mutex::new(Some(step_id)),
        }
    }
}

#[async_trait]
impl FlowEventStore for FailStepCompletionStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope, FlowError> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope, FlowError> {
        if let FlowEvent::StepCompleted { step_id, .. } = &event {
            let mut fail = self
                .step_id
                .lock()
                .map_err(|_| FlowError::Store("fault-injection lock poisoned".into()))?;
            if fail.as_deref() == Some(step_id.as_str()) {
                fail.take();
                return Err(FlowError::Store(format!(
                    "injected loss before persisting {run_id} step {step_id} completion"
                )));
            }
        }
        self.inner
            .append_if_sequence(run_id, expected_sequence, event)
            .await
    }

    async fn list(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>, FlowError> {
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> Result<Vec<String>, FlowError> {
        self.inner.list_run_ids().await
    }
}

#[test]
fn workflow_identity_constants_remain_distinct_and_versioned() {
    assert_eq!(LEGACY_OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION, "1");
    assert_eq!(OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION, "2");
    assert_eq!(
        [
            OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME,
            OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME,
            OBJECT_NAMESPACE_DELETE_WORKFLOW_NAME,
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len(),
        3
    );
    assert_eq!(flow_workflow_identities().count(), 6);
}
