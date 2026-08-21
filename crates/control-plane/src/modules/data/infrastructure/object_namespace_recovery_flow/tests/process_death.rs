use super::*;
use crate::infrastructure::{
    cloud_runtime_build_compatibility, CURRENT_CLOUD_FLOW_RUNTIME_BUILD_ID,
};
use a3s_flow::{PostgresEventStore, RuntimeBuildId, WorkflowRunSnapshot};
use a3s_orm::PostgresExecutor;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

const POSTGRES_ENV: &str = "A3S_CLOUD_TEST_POSTGRES_URL";
const PROBE_PARENT_ENV: &str = "A3S_CLOUD_OBJECT_PAGE_PROBE_PARENT";
const PROBE_POSTGRES_ENV: &str = "A3S_CLOUD_OBJECT_PAGE_PROBE_POSTGRES_URL";
const PROBE_STATE_ENV: &str = "A3S_CLOUD_OBJECT_PAGE_PROBE_STATE";
const PROBE_DOCUMENT_ENV: &str = "A3S_CLOUD_OBJECT_PAGE_PROBE_DOCUMENT";
const PROBE_TARGET_ENV: &str = "A3S_CLOUD_OBJECT_PAGE_PROBE_TARGET";
const PROBE_MARKER_ENV: &str = "A3S_CLOUD_OBJECT_PAGE_PROBE_MARKER";
const PROBE_RESUME_ENV: &str = "A3S_CLOUD_OBJECT_PAGE_PROBE_RESUME";
const PROBE_TEST: &str = "modules::data::infrastructure::object_namespace_recovery_flow::tests::process_death::object_namespace_page_process_death_probe";

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProbeDocument {
    run_id: String,
    spec: WorkflowSpec,
    input: serde_json::Value,
}

struct PersistentFixture {
    state_dir: tempfile::TempDir,
    organization_id: OrganizationId,
    target_namespace_id: StorageNamespaceId,
    source_binding: ObjectNamespaceFlowBinding,
    target_binding: ObjectNamespaceFlowBinding,
    source_namespace: Arc<dyn IObjectNamespace>,
    recovery_namespace: Arc<dyn IObjectNamespace>,
    target_namespace: Arc<dyn IObjectNamespace>,
}

impl PersistentFixture {
    fn new() -> TestResult<Self> {
        let state_dir = tempfile::tempdir()?;
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let source_namespace_id = StorageNamespaceId::new();
        let target_namespace_id = StorageNamespaceId::new();
        let profile =
            ObjectNamespaceProviderProfile::from_spec(ObjectNamespaceProviderProfileSpec {
                endpoint: "https://s3.example.com".into(),
                region: "us-east-1".into(),
                bucket: "a3s-process-death-test".into(),
                prefix: "tests/object-pages".into(),
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
        let source_namespace = persistent_namespace(state_dir.path(), "live/source")?;
        let recovery_namespace = persistent_namespace(state_dir.path(), "recovery/source")?;
        let target_namespace = persistent_namespace(state_dir.path(), "live/target")?;
        Ok(Self {
            state_dir,
            organization_id,
            target_namespace_id,
            source_binding,
            target_binding,
            source_namespace,
            recovery_namespace,
            target_namespace,
        })
    }

    fn runtime(&self, document: &ProbeDocument) -> TestResult<ObjectNamespaceRecoveryFlowRuntime> {
        runtime_for_document(self.state_dir.path(), document)
    }
}

struct IsolatedFlowDatabase {
    admin_url: String,
    database_name: String,
    database_url: String,
}

impl IsolatedFlowDatabase {
    async fn create(admin_url: &str) -> TestResult<Self> {
        let database_name = format!("a3s_cloud_object_pages_{}", Uuid::new_v4().simple());
        let mut database_url = url::Url::parse(admin_url)?;
        database_url.set_path(&format!("/{database_name}"));
        let admin = PostgresExecutor::connect_no_tls(admin_url, 2)?;
        let connection = admin.pool().get().await?;
        connection
            .batch_execute(&format!("create database \"{database_name}\""))
            .await?;
        drop(connection);
        drop(admin);
        let database = PostgresExecutor::connect_no_tls(database_url.as_str(), 2)?;
        database
            .pool()
            .get()
            .await?
            .batch_execute("create schema a3s_flow")
            .await?;
        drop(database);
        Ok(Self {
            admin_url: admin_url.into(),
            database_name,
            database_url: database_url.to_string(),
        })
    }

    async fn cleanup(&self) -> TestResult {
        let admin = PostgresExecutor::connect_no_tls(&self.admin_url, 2)?;
        admin
            .pool()
            .get()
            .await?
            .batch_execute(&format!(
                "drop database if exists \"{}\" with (force)",
                self.database_name
            ))
            .await?;
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_object_namespace_pages_survive_process_death() {
    let Some(admin_url) = std::env::var(POSTGRES_ENV).ok() else {
        return;
    };
    let database = IsolatedFlowDatabase::create(&admin_url)
        .await
        .expect("create isolated object-page Flow database");
    let result = exercise_process_death_pages(&database.database_url).await;
    let cleanup = database.cleanup().await;
    if let Err(error) = result {
        panic!("object namespace page process-death gate failed: {error}");
    }
    cleanup.expect("clean isolated object-page Flow database");
}

#[tokio::test]
#[ignore = "private subprocess used only by the object namespace page process-death gate"]
async fn object_namespace_page_process_death_probe() {
    run_probe()
        .await
        .expect("run object namespace page process-death probe");
}

async fn exercise_process_death_pages(postgres_url: &str) -> TestResult {
    let fixture = PersistentFixture::new()?;
    for index in 0..33 {
        put(
            &fixture.source_namespace,
            &format!("state/{index:04}.bin"),
            format!("value-{index:04}").as_bytes(),
        )
        .await?;
    }
    let sealed_at = canonical_timestamp(Utc::now());
    let seal_id = OperationId::new();
    let seal_request =
        ObjectNamespaceRecoveryOperationRequest::seal(SealObjectNamespaceOperationInput {
            operation_id: seal_id,
            organization_id: fixture.organization_id,
            source: fixture.source_binding.clone(),
            previous_recovery_point: None,
            writer_epoch: 7,
            writer_fence_receipt_digest: digest('8'),
            sealed_at,
        })?;
    let sealed: SealObjectNamespaceOperationOutput = crash_and_recover(
        postgres_url,
        &fixture,
        &seal_request,
        "seal-snapshot-0001",
        false,
    )
    .await?;

    let policy = retention_policy()?;
    let restore_plan = ObjectNamespaceRestorePlan::for_recovery_point(
        &sealed.recovery_point,
        fixture.target_namespace_id,
        fixture.target_binding.provider_profile.digest().clone(),
        &policy,
        sealed_at + Duration::seconds(1),
    )?;
    let restore_id = OperationId::new();
    let restore_request =
        ObjectNamespaceRecoveryOperationRequest::restore(RestoreObjectNamespaceOperationInput {
            operation_id: restore_id,
            organization_id: fixture.organization_id,
            source: fixture.source_binding.clone(),
            target: fixture.target_binding.clone(),
            recovery_point: sealed.recovery_point.clone(),
            restore_plan: restore_plan.clone(),
            retention_policy: policy.clone(),
        })?;
    let restored: RestoreObjectNamespaceOperationOutput = crash_and_recover(
        postgres_url,
        &fixture,
        &restore_request,
        "restore-apply-0001",
        false,
    )
    .await?;

    let deletion_plan = ObjectNamespaceDeletionPlan::after_verified_restore(
        &sealed.recovery_point,
        &restore_plan,
        &restored.restore_evidence,
        &policy,
        digest('9'),
        digest('a'),
        restored.restore_evidence.verified_at + Duration::seconds(1),
    )?;
    let delete_id = OperationId::new();
    let delete_request =
        ObjectNamespaceRecoveryOperationRequest::delete(DeleteObjectNamespaceOperationInput {
            operation_id: delete_id,
            organization_id: fixture.organization_id,
            source: fixture.source_binding.clone(),
            retained_restore: fixture.target_binding.clone(),
            recovery_point: sealed.recovery_point,
            restore_plan,
            restore_evidence: restored.restore_evidence,
            deletion_plan,
            retention_policy: policy,
        })?;
    let _: DeleteObjectNamespaceOperationOutput = crash_and_recover(
        postgres_url,
        &fixture,
        &delete_request,
        "delete-recovery-0001",
        true,
    )
    .await?;

    if !fixture
        .source_namespace
        .list(None, 64, 8192)
        .await?
        .is_empty()
        || !fixture
            .recovery_namespace
            .list(None, 128, 32 * 1024)
            .await?
            .is_empty()
        || fixture.target_namespace.list(None, 64, 8192).await?.len() != 33
    {
        return Err("object namespace process-death recovery lost final state invariants".into());
    }
    println!(
        "A3S_CLOUD_OBJECT_NAMESPACE_PAGE_PROCESS_DEATH_CERTIFIED boundaries=3 sigkills=3 objects=33 store=postgresql"
    );
    Ok(())
}

async fn crash_and_recover<T: serde::de::DeserializeOwned>(
    postgres_url: &str,
    fixture: &PersistentFixture,
    request: &crate::modules::operations::OperationRequest,
    target: &str,
    resume_due_wait: bool,
) -> TestResult<T> {
    let document = ProbeDocument {
        run_id: request.id.to_string(),
        spec: process_death_workflow_spec(request)?,
        input: request.input.clone(),
    };
    if resume_due_wait {
        let engine = recovery_engine(postgres_url, fixture, &document).await?;
        engine
            .start_with_id(
                document.run_id.clone(),
                document.spec.clone(),
                document.input.clone(),
            )
            .await?;
        let snapshot = engine.snapshot(&document.run_id).await?;
        if snapshot.status != WorkflowRunStatus::Suspended {
            return Err("object namespace delete did not persist its grace wait".into());
        }
    }
    let document_path = fixture
        .state_dir
        .path()
        .join(format!("{}.json", request.id));
    std::fs::write(&document_path, serde_json::to_vec(&document)?)?;
    let marker = fixture
        .state_dir
        .path()
        .join(format!("crash-{target}.json"));
    assert_completion_count(postgres_url, &document.run_id, target, 0).await?;
    let mut probe = CrashProbe::start(
        postgres_url,
        fixture.state_dir.path(),
        &document_path,
        target,
        &marker,
        resume_due_wait,
    )?;
    wait_for_marker(&mut probe, &marker, target).await?;
    assert_completion_count(postgres_url, &document.run_id, target, 0).await?;
    require_killed(probe.kill_and_wait()?)?;
    assert_completion_count(postgres_url, &document.run_id, target, 0).await?;

    let engine = recovery_engine(postgres_url, fixture, &document).await?;
    engine
        .start_with_id(
            document.run_id.clone(),
            document.spec.clone(),
            document.input.clone(),
        )
        .await?;
    let mut snapshot = engine.snapshot(&document.run_id).await?;
    if resume_due_wait && snapshot.status != WorkflowRunStatus::Completed {
        engine
            .resume_due_waits(Utc::now() + Duration::days(1))
            .await?;
        snapshot = engine.snapshot(&document.run_id).await?;
    }
    require_completed(&snapshot, target)?;
    assert_completion_count(postgres_url, &document.run_id, target, 1).await?;
    serde_json::from_value(snapshot.output.ok_or("completed Flow omitted output")?)
        .map_err(Into::into)
}

async fn run_probe() -> TestResult {
    if required_env(PROBE_PARENT_ENV)? != "1" {
        return Err("object namespace crash probe requires its private parent marker".into());
    }
    let postgres_url = required_env(PROBE_POSTGRES_ENV)?;
    let state_dir = PathBuf::from(required_env(PROBE_STATE_ENV)?);
    let document = serde_json::from_slice::<ProbeDocument>(&std::fs::read(required_env(
        PROBE_DOCUMENT_ENV,
    )?)?)?;
    let target = required_env(PROBE_TARGET_ENV)?;
    let marker = PathBuf::from(required_env(PROBE_MARKER_ENV)?);
    let store = CrashBeforeCompletionStore::new(
        postgres_store(&postgres_url).await?,
        target.clone(),
        marker,
    );
    let engine = FlowEngine::builder(Arc::new(runtime_for_document(&state_dir, &document)?))
        .with_store(Arc::new(store))
        .with_runtime_build_compatibility(cloud_runtime_build_compatibility()?)
        .build();
    engine
        .start_with_id(document.run_id, document.spec, document.input)
        .await?;
    if required_env(PROBE_RESUME_ENV)? == "1" {
        engine
            .resume_due_waits(Utc::now() + Duration::days(1))
            .await?;
    }
    Err(format!("object namespace crash probe returned before {target} completion").into())
}

fn process_death_workflow_spec(
    request: &crate::modules::operations::OperationRequest,
) -> TestResult<WorkflowSpec> {
    Ok(workflow_spec(request)
        .with_runtime_build(RuntimeBuildId::new(CURRENT_CLOUD_FLOW_RUNTIME_BUILD_ID)?))
}

async fn recovery_engine(
    postgres_url: &str,
    fixture: &PersistentFixture,
    document: &ProbeDocument,
) -> TestResult<FlowEngine> {
    Ok(FlowEngine::builder(Arc::new(fixture.runtime(document)?))
        .with_store(Arc::new(postgres_store(postgres_url).await?))
        .with_runtime_build_compatibility(cloud_runtime_build_compatibility()?)
        .build())
}

fn runtime_for_document(
    root: &Path,
    document: &ProbeDocument,
) -> TestResult<ObjectNamespaceRecoveryFlowRuntime> {
    let resolver = Arc::new(InMemoryAccessResolver::default());
    match document.spec.name.as_str() {
        OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME => {
            let input: SealObjectNamespaceOperationInput =
                serde_json::from_value(document.input.clone())?;
            register_source(root, &resolver, &input.source)?;
        }
        OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME => {
            let input: RestoreObjectNamespaceOperationInput =
                serde_json::from_value(document.input.clone())?;
            register_source(root, &resolver, &input.source)?;
            register_target(root, &resolver, &input.target)?;
        }
        OBJECT_NAMESPACE_DELETE_WORKFLOW_NAME => {
            let input: DeleteObjectNamespaceOperationInput =
                serde_json::from_value(document.input.clone())?;
            register_source(root, &resolver, &input.source)?;
            register_target(root, &resolver, &input.retained_restore)?;
        }
        name => return Err(format!("unknown object namespace probe workflow {name}").into()),
    }
    Ok(ObjectNamespaceRecoveryFlowRuntime::with_resolver(
        resolver,
        ObjectNamespaceRecoveryExecutor::new(64, 1024, 8192, 8192)?,
    ))
}

fn register_source(
    root: &Path,
    resolver: &InMemoryAccessResolver,
    binding: &ObjectNamespaceFlowBinding,
) -> TestResult {
    let namespace_id = binding.credentials.spec().namespace_id;
    resolver.register_live(namespace_id, persistent_namespace(root, "live/source")?);
    resolver.register_recovery(namespace_id, persistent_namespace(root, "recovery/source")?);
    Ok(())
}

fn register_target(
    root: &Path,
    resolver: &InMemoryAccessResolver,
    binding: &ObjectNamespaceFlowBinding,
) -> TestResult {
    resolver.register_live(
        binding.credentials.spec().namespace_id,
        persistent_namespace(root, "live/target")?,
    );
    Ok(())
}

fn persistent_namespace(root: &Path, prefix: &str) -> TestResult<Arc<dyn IObjectNamespace>> {
    Ok(Arc::new(ImmutableObjectClient::local(root, prefix)?))
}

async fn postgres_store(postgres_url: &str) -> TestResult<PostgresEventStore> {
    let mut url = url::Url::parse(postgres_url)?;
    if url.query_pairs().any(|(key, _)| key == "options") {
        return Err("object namespace process-death URL already defines options".into());
    }
    url.query_pairs_mut()
        .append_pair("options", "-csearch_path=a3s_flow");
    Ok(PostgresEventStore::connect(url.as_str()).await?)
}

async fn assert_completion_count(
    postgres_url: &str,
    run_id: &str,
    target: &str,
    expected: usize,
) -> TestResult {
    let store = postgres_store(postgres_url).await?;
    let history = match store.list(run_id).await {
        Ok(history) => history,
        Err(FlowError::RunNotFound(_)) if expected == 0 => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let actual = history
        .iter()
        .filter(|event| {
            matches!(&event.event, FlowEvent::StepCompleted { step_id, .. } if step_id == target)
        })
        .count();
    if actual != expected {
        return Err(format!(
            "object namespace Flow has {actual} completions for {target}, expected {expected}"
        )
        .into());
    }
    Ok(())
}

fn require_completed(snapshot: &WorkflowRunSnapshot, target: &str) -> TestResult {
    if snapshot.status != WorkflowRunStatus::Completed {
        return Err(format!(
            "object namespace Flow recovered {target} into {:?}",
            snapshot.status
        )
        .into());
    }
    Ok(())
}

struct CrashBeforeCompletionStore {
    inner: PostgresEventStore,
    target: String,
    marker: PathBuf,
}

impl CrashBeforeCompletionStore {
    fn new(inner: PostgresEventStore, target: String, marker: PathBuf) -> Self {
        Self {
            inner,
            target,
            marker,
        }
    }

    async fn pause(&self, run_id: &str, expected_sequence: u64) -> Result<(), FlowError> {
        let temporary = self.marker.with_extension("tmp");
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .await
            .map_err(|error| FlowError::Store(format!("create crash marker: {error}")))?;
        file.write_all(&serde_json::to_vec(&serde_json::json!({
            "runId": run_id,
            "stepId": self.target,
            "expectedSequence": expected_sequence,
        }))?)
        .await
        .map_err(|error| FlowError::Store(format!("write crash marker: {error}")))?;
        file.sync_all()
            .await
            .map_err(|error| FlowError::Store(format!("sync crash marker: {error}")))?;
        tokio::fs::rename(temporary, &self.marker)
            .await
            .map_err(|error| FlowError::Store(format!("publish crash marker: {error}")))?;
        std::future::pending::<Result<(), FlowError>>().await
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeCompletionStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope, FlowError> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope, FlowError> {
        if matches!(&event, FlowEvent::StepCompleted { step_id, .. } if step_id == &self.target) {
            self.pause(run_id, expected_sequence).await?;
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

struct CrashProbe(Option<Child>);

impl CrashProbe {
    #[allow(clippy::too_many_arguments)]
    fn start(
        postgres_url: &str,
        state_dir: &Path,
        document: &Path,
        target: &str,
        marker: &Path,
        resume_due_wait: bool,
    ) -> TestResult<Self> {
        Ok(Self(Some(
            Command::new(std::env::current_exe()?)
                .arg(PROBE_TEST)
                .arg("--exact")
                .arg("--ignored")
                .arg("--nocapture")
                .arg("--test-threads=1")
                .env(PROBE_PARENT_ENV, "1")
                .env(PROBE_POSTGRES_ENV, postgres_url)
                .env(PROBE_STATE_ENV, state_dir)
                .env(PROBE_DOCUMENT_ENV, document)
                .env(PROBE_TARGET_ENV, target)
                .env(PROBE_MARKER_ENV, marker)
                .env(PROBE_RESUME_ENV, if resume_due_wait { "1" } else { "0" })
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()?,
        )))
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.0
            .as_mut()
            .ok_or_else(|| std::io::Error::other("object namespace crash probe disappeared"))?
            .try_wait()
    }

    fn kill_and_wait(mut self) -> std::io::Result<ExitStatus> {
        let mut child = self
            .0
            .take()
            .ok_or_else(|| std::io::Error::other("object namespace crash probe disappeared"))?;
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        child.kill()?;
        child.wait()
    }
}

impl Drop for CrashProbe {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

async fn wait_for_marker(probe: &mut CrashProbe, marker: &Path, target: &str) -> TestResult {
    let deadline = Instant::now() + std::time::Duration::from_secs(60);
    loop {
        if marker.is_file() {
            return Ok(());
        }
        if let Some(status) = probe.try_wait()? {
            return Err(format!(
                "object namespace crash probe exited with {status} before {target}"
            )
            .into());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "object namespace crash probe did not reach {target} in 60 seconds"
            )
            .into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

fn require_killed(status: ExitStatus) -> TestResult {
    if status.success() {
        return Err("object namespace crash probe exited successfully".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal() != Some(9) {
            return Err(format!("object namespace crash probe exited with {status}").into());
        }
    }
    Ok(())
}

fn required_env(name: &str) -> Result<String, std::io::Error> {
    std::env::var(name)
        .map_err(|_| std::io::Error::other(format!("object namespace crash probe omitted {name}")))
}
