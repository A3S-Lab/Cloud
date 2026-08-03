use super::evidence::PersistentEvidenceGenerator;
use a3s_cloud_contracts::{artifact_uri, NodeBoxBuildOutput, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE};
use a3s_cloud_control_plane::modules::artifacts::application::{
    BUILD_WORKFLOW_NAME, BUILD_WORKFLOW_VERSION,
};
use a3s_cloud_control_plane::modules::artifacts::{
    BuildArtifact, BuildArtifactPublicationError, BuildFlowConfig, BuildFlowConfigOptions,
    BuildFlowRuntime, BuildFlowRuntimeDependencies, BuildInputPreparationError,
    BuildOutputValidationError, BuildRun, BuildSource, BuildSourceResolutionError,
    IBuildArtifactPublisher, IBuildInputPreparer, IBuildOutputValidator, IBuildSourceResolver,
    LocalBuildEvidenceSigner, OciDescriptor, OciPublicationRequest, OciPublicationTarget,
    PostgresBuildRunRepository, PreparedBuildInput, PublishedOciArtifact, ValidatedOciBuildOutput,
};
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    BuildRunId, OrganizationId, RepositoryError,
};
use a3s_cloud_control_plane::modules::sources::domain::{BuildRecipe, ISourceRevisionRepository};
use a3s_cloud_control_plane::modules::sources::PostgresSourceRevisionRepository;
use a3s_flow::{
    FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, PostgresEventStore, WorkflowSpec,
};
use a3s_orm::PostgresExecutor;
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

const ACTIONS_FILE: &str = "build-flow-actions.log";

pub(super) async fn build_runtime(
    executor: PostgresExecutor,
    state_dir: &Path,
) -> Result<BuildFlowRuntime, Box<dyn std::error::Error>> {
    let actions = ActionRecorder::new(state_dir.join(ACTIONS_FILE));
    let sources = Arc::new(PostgresSourceRevisionRepository::new(executor.clone()));
    let nodes = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let signer = Arc::new(
        LocalBuildEvidenceSigner::load_or_create(state_dir.join("build-evidence-ed25519.pk8"))
            .await?,
    );
    let evidence = Arc::new(PersistentEvidenceGenerator::new(actions.clone(), signer));
    Ok(BuildFlowRuntime::new(
        BuildFlowRuntimeDependencies {
            builds: Arc::new(PostgresBuildRunRepository::new(executor)),
            sources: Arc::new(PostgresBuildSourceResolver { sources }),
            inputs: Arc::new(PersistentInputPreparer {
                actions: actions.clone(),
                artifact: input_artifact()?,
            }),
            outputs: Arc::new(PersistentOutputValidator {
                actions: actions.clone(),
                artifact: output_artifact()?,
            }),
            publisher: Arc::new(PersistentPublisher {
                actions: actions.clone(),
            }),
            evidence,
            nodes: nodes.clone(),
            node_control: nodes,
        },
        BuildFlowConfig::new(BuildFlowConfigOptions {
            heartbeat_timeout_ms: 60 * 60 * 1_000,
            command_ttl_ms: 60 * 60 * 1_000,
            execution_timeout_ms: 30 * 60 * 1_000,
            observation_poll_ms: 1,
            convergence_timeout_ms: 60 * 60 * 1_000,
            cleanup_timeout_ms: 60 * 60 * 1_000,
            publication_timeout_ms: 60 * 60 * 1_000,
            output_max_bytes: 128 * 1024 * 1024,
            cache_max_bytes: 128 * 1024 * 1024,
        })?,
    ))
}

pub(super) async fn postgres_flow_store(
    postgres_url: &str,
) -> Result<PostgresEventStore, Box<dyn std::error::Error>> {
    Ok(PostgresEventStore::connect(scoped_flow_url(postgres_url)?).await?)
}

fn scoped_flow_url(postgres_url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut url = url::Url::parse(postgres_url)?;
    if url.query_pairs().any(|(key, _)| key == "options") {
        return Err("Build Flow PostgreSQL fixture URL already defines options".into());
    }
    url.query_pairs_mut()
        .append_pair("options", "-csearch_path=a3s_flow");
    Ok(url.to_string())
}

pub(super) fn workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        BUILD_WORKFLOW_NAME,
        BUILD_WORKFLOW_VERSION,
        "a3s-cloud",
        "main",
    )
}

pub(super) fn flow_input(
    organization_id: OrganizationId,
    build_run_id: BuildRunId,
) -> serde_json::Value {
    serde_json::json!({
        "organizationId": organization_id,
        "buildRunId": build_run_id,
    })
}

pub(super) fn digest(fill: char) -> String {
    format!("sha256:{}", fill.to_string().repeat(64))
}

fn input_artifact() -> Result<BuildArtifact, String> {
    artifact('1', 4096)
}

pub(super) fn output_artifact() -> Result<BuildArtifact, String> {
    artifact('2', 8192)
}

fn artifact(fill: char, size_bytes: u64) -> Result<BuildArtifact, String> {
    let digest = digest(fill);
    BuildArtifact::new(
        artifact_uri(&digest)?,
        digest,
        NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
        size_bytes,
    )
}

#[derive(Clone)]
pub(super) struct ActionRecorder {
    path: PathBuf,
}

impl ActionRecorder {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(super) fn record(&self, action: &str) -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{action}")?;
        file.sync_all()
    }
}

pub(super) fn action_counts(state_dir: &Path) -> std::io::Result<BTreeMap<String, usize>> {
    let actions = std::fs::read_to_string(state_dir.join(ACTIONS_FILE))?;
    let mut counts = BTreeMap::new();
    for action in actions.lines() {
        *counts.entry(action.to_owned()).or_default() += 1;
    }
    Ok(counts)
}

struct PostgresBuildSourceResolver {
    sources: Arc<PostgresSourceRevisionRepository>,
}

#[async_trait]
impl IBuildSourceResolver for PostgresBuildSourceResolver {
    async fn resolve(&self, build: &BuildRun) -> Result<BuildSource, BuildSourceResolutionError> {
        let source_revision_id = build.source_revision_id().ok_or_else(|| {
            BuildSourceResolutionError::Invalid(
                "persistent Build Flow fixture requires an external source revision".into(),
            )
        })?;
        let revision = self
            .sources
            .find(build.organization_id, source_revision_id)
            .await
            .map_err(map_source_repository_error)?;
        BuildSource::from_external_revision(&revision)
            .map_err(BuildSourceResolutionError::Integrity)
    }
}

fn map_source_repository_error(error: RepositoryError) -> BuildSourceResolutionError {
    match error {
        RepositoryError::NotFound => BuildSourceResolutionError::NotFound,
        RepositoryError::Conflict(_) | RepositoryError::IdempotencyConflict => {
            BuildSourceResolutionError::Conflict
        }
        RepositoryError::Storage(message) => BuildSourceResolutionError::Storage(message),
    }
}

struct PersistentInputPreparer {
    actions: ActionRecorder,
    artifact: BuildArtifact,
}

#[async_trait]
impl IBuildInputPreparer for PersistentInputPreparer {
    async fn prepare(
        &self,
        build: &BuildRun,
        source: &BuildSource,
    ) -> Result<PreparedBuildInput, BuildInputPreparationError> {
        if build.organization_id != source.organization_id || build.subject != source.subject {
            return Err(BuildInputPreparationError::Conflict);
        }
        self.actions
            .record("input.prepare")
            .map_err(|error| BuildInputPreparationError::Storage(error.to_string()))?;
        Ok(PreparedBuildInput {
            source_content_digest: digest('d'),
            artifact: self.artifact.clone(),
        })
    }

    async fn remove(&self, _build: &BuildRun) -> Result<(), BuildInputPreparationError> {
        self.actions
            .record("input.remove")
            .map_err(|error| BuildInputPreparationError::Storage(error.to_string()))
    }
}

struct PersistentOutputValidator {
    actions: ActionRecorder,
    artifact: BuildArtifact,
}

#[async_trait]
impl IBuildOutputValidator for PersistentOutputValidator {
    async fn validate(
        &self,
        output: &NodeBoxBuildOutput,
        recipe: &BuildRecipe,
    ) -> Result<ValidatedOciBuildOutput, BuildOutputValidationError> {
        self.actions
            .record("output.validate")
            .map_err(|error| BuildOutputValidationError::Storage(error.to_string()))?;
        output
            .validate()
            .map_err(BuildOutputValidationError::Integrity)?;
        if output.artifact.artifact.uri != self.artifact.uri
            || output.artifact.artifact.digest != self.artifact.digest
            || output.artifact.artifact.media_type != self.artifact.media_type
            || output.artifact.size_bytes != self.artifact.size_bytes
        {
            return Err(BuildOutputValidationError::Integrity(
                "persistent Box output changed identity".into(),
            ));
        }
        Ok(ValidatedOciBuildOutput {
            artifact: self.artifact.clone(),
            descriptor: OciDescriptor::new(
                &output.descriptor.media_type,
                &output.descriptor.digest,
                output.descriptor.size,
            )
            .map_err(BuildOutputValidationError::Invalid)?,
            platforms: recipe.platforms().to_vec(),
            content_bytes: output.content_bytes,
            blob_count: output.blob_count as usize,
        })
    }
}

struct PersistentPublisher {
    actions: ActionRecorder,
}

#[async_trait]
impl IBuildArtifactPublisher for PersistentPublisher {
    fn target_for(
        &self,
        build: &BuildRun,
    ) -> Result<OciPublicationTarget, BuildArtifactPublicationError> {
        self.actions
            .record("publication.target")
            .map_err(|error| BuildArtifactPublicationError::Storage(error.to_string()))?;
        let output = build.output.as_ref().ok_or_else(|| {
            BuildArtifactPublicationError::Invalid("persistent build output is missing".into())
        })?;
        OciPublicationTarget::new(
            "registry.example",
            format!("a3s-cloud/builds/{}", build.id),
            output.descriptor.clone(),
        )
        .map_err(BuildArtifactPublicationError::Invalid)
    }

    async fn find(
        &self,
        _request: &OciPublicationRequest,
    ) -> Result<Option<PublishedOciArtifact>, BuildArtifactPublicationError> {
        self.actions
            .record("publication.find")
            .map_err(|error| BuildArtifactPublicationError::Storage(error.to_string()))?;
        Ok(None)
    }

    async fn publish(
        &self,
        request: &OciPublicationRequest,
    ) -> Result<PublishedOciArtifact, BuildArtifactPublicationError> {
        self.actions
            .record("publication.publish")
            .map_err(|error| BuildArtifactPublicationError::Storage(error.to_string()))?;
        Ok(PublishedOciArtifact::from_target(&request.target))
    }
}

pub(super) struct CrashBeforeStepCompletionStore {
    inner: PostgresEventStore,
    target: String,
    marker: PathBuf,
}

impl CrashBeforeStepCompletionStore {
    pub(super) fn new(inner: PostgresEventStore, target: String, marker: PathBuf) -> Self {
        Self {
            inner,
            target,
            marker,
        }
    }

    async fn pause(&self, run_id: &str, expected_sequence: u64) -> Result<(), FlowError> {
        let temporary_marker = self.marker.with_extension("tmp");
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_marker)
            .await
            .map_err(|error| {
                FlowError::Store(format!("could not create process-death marker: {error}"))
            })?;
        let marker = serde_json::to_vec(&serde_json::json!({
            "runId": run_id,
            "stepId": self.target,
            "expectedSequence": expected_sequence,
        }))
        .map_err(|error| FlowError::Store(format!("could not encode crash marker: {error}")))?;
        file.write_all(&marker)
            .await
            .map_err(|error| FlowError::Store(format!("could not write crash marker: {error}")))?;
        file.sync_all()
            .await
            .map_err(|error| FlowError::Store(format!("could not sync crash marker: {error}")))?;
        tokio::fs::rename(&temporary_marker, &self.marker)
            .await
            .map_err(|error| {
                FlowError::Store(format!("could not publish process-death marker: {error}"))
            })?;
        std::future::pending::<Result<(), FlowError>>().await
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeStepCompletionStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope, FlowError> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope, FlowError> {
        let is_target = matches!(
            &event,
            FlowEvent::StepCompleted { step_id, .. } if step_id == &self.target
        );
        if is_target {
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
