use super::super::{
    BuildFlowConfig, BuildFlowConfigOptions, BuildFlowRuntime, BuildFlowRuntimeDependencies,
};
use crate::modules::artifacts::application::{
    BUILD_WORKFLOW_NAME, BUILD_WORKFLOW_VERSION, LEGACY_BUILD_WORKFLOW_VERSION,
};
use crate::modules::artifacts::domain::{
    BuildArtifact, BuildArtifactPublicationError, BuildInputPreparationError,
    BuildOutputValidationError, BuildRun, IBuildArtifactPublisher, IBuildInputPreparer,
    IBuildOutputValidator, IBuildRunRepository, OciDescriptor, OciPublicationRequest,
    OciPublicationTarget, PreparedBuildInput, PublishedOciArtifact, ValidatedOciBuildOutput,
};
use crate::modules::artifacts::infrastructure::InMemoryBuildRunRepository;
use crate::modules::fleet::domain::entities::EnrollmentToken;
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeEnrollmentDraft, NodeHeartbeatUpdate,
};
use crate::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName,
};
use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
use crate::modules::shared_kernel::domain::{
    EnrollmentTokenId, EnvironmentId, IdempotencyRequest, NodeId, OrganizationId, ProjectId,
    SourceRevisionId,
};
use crate::modules::sources::domain::{
    AcceptSourceRevision, BuildRecipe, ExternalSourceRevision, GitCommitSha, GitProvider,
    GitRepository, ISourceRevisionRepository, NewExternalSourceRevision,
};
use crate::modules::sources::infrastructure::persistence::InMemorySourceRevisionRepository;
use a3s_cloud_contracts::{
    artifact_uri, DomainEventEnvelope, NodeCommandLeaseRequest, NodeHeartbeat,
    NodeObservationBatch, RuntimeObservationReport, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_flow::{
    FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, InMemoryEventStore, WorkflowSpec,
};
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, MountKind, NetworkMode, ResourceControl, RuntimeCapabilities,
    RuntimeFeature, RuntimeObservation, RuntimeOutputArtifact, RuntimeUnitClass, RuntimeUnitState,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;
use uuid::Uuid;

pub(super) const BUILDER_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";

pub(super) struct BuildFixture {
    pub organization_id: OrganizationId,
    pub build: BuildRun,
    pub builds: Arc<InMemoryBuildRunRepository>,
    pub sources: Arc<InMemorySourceRevisionRepository>,
    pub nodes: Arc<InMemoryNodeRepository>,
    pub inputs: Arc<RecordingInputPreparer>,
    pub outputs: Arc<RecordingOutputValidator>,
    pub publisher: Arc<RecordingPublisher>,
    pub node_id: NodeId,
    pub agent_instance_id: Uuid,
    pub capabilities: RuntimeCapabilities,
    pub runtime: BuildFlowRuntime,
}

impl BuildFixture {
    pub(super) async fn create(
        output_failure: Option<BuildOutputValidationError>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let base = Utc::now() - Duration::seconds(1);
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let source_revision_id = SourceRevisionId::new();
        let revision = revision(
            organization_id,
            project_id,
            environment_id,
            source_revision_id,
            base,
        )?;
        let sources = Arc::new(InMemorySourceRevisionRepository::new());
        accept_revision(&sources, revision.clone()).await?;
        let builds = Arc::new(InMemoryBuildRunRepository::new());
        builds
            .add_source_revision(
                organization_id,
                project_id,
                environment_id,
                source_revision_id,
                base,
            )
            .await;
        let build = builds
            .reserve_pending(1, base)
            .await?
            .pop()
            .ok_or("build reservation did not produce a build")?;
        let nodes = Arc::new(InMemoryNodeRepository::new());
        let missing_capabilities = build_capabilities(false);
        ready_node(
            &nodes,
            organization_id,
            base,
            "build-node-missing-index",
            missing_capabilities,
        )
        .await?;
        let capabilities = build_capabilities(true);
        let (node_id, agent_instance_id) = ready_node(
            &nodes,
            organization_id,
            base,
            "build-node-ready",
            capabilities.clone(),
        )
        .await?;
        let input_artifact = artifact('1', 4096)?;
        let runtime_output = artifact('2', 8192)?;
        let inputs = Arc::new(RecordingInputPreparer::new(input_artifact));
        let outputs = Arc::new(RecordingOutputValidator::new(
            runtime_output,
            output_failure,
        ));
        let publisher = Arc::new(RecordingPublisher::new());
        let build_port: Arc<dyn IBuildRunRepository> = builds.clone();
        let source_port: Arc<dyn ISourceRevisionRepository> = sources.clone();
        let input_port: Arc<dyn IBuildInputPreparer> = inputs.clone();
        let output_port: Arc<dyn IBuildOutputValidator> = outputs.clone();
        let publisher_port: Arc<dyn IBuildArtifactPublisher> = publisher.clone();
        let node_port: Arc<dyn INodeRepository> = nodes.clone();
        let control_port: Arc<dyn INodeControlRepository> = nodes.clone();
        let runtime = BuildFlowRuntime::new(
            BuildFlowRuntimeDependencies {
                builds: build_port,
                sources: source_port,
                inputs: input_port,
                outputs: output_port,
                publisher: publisher_port,
                nodes: node_port,
                node_control: control_port,
            },
            config()?,
        );
        Ok(Self {
            organization_id,
            build,
            builds,
            sources,
            nodes,
            inputs,
            outputs,
            publisher,
            node_id,
            agent_instance_id,
            capabilities,
            runtime,
        })
    }

    pub(super) fn input(&self) -> serde_json::Value {
        serde_json::json!({
            "organizationId": self.organization_id,
            "buildRunId": self.build.id,
        })
    }
}

pub(super) fn config() -> Result<BuildFlowConfig, String> {
    let digest = format!("sha256:{}", "a".repeat(64));
    BuildFlowConfig::new(BuildFlowConfigOptions {
        builder: ArtifactRef {
            uri: format!("oci://docker.io/moby/buildkit@{digest}"),
            digest,
            media_type: BUILDER_MEDIA_TYPE.into(),
        },
        buildkit_socket_volume_id: "a3s-cloud-buildkit-v0-31-2".into(),
        heartbeat_timeout_ms: 5_000,
        command_ttl_ms: 30_000,
        execution_timeout_ms: 10_000,
        observation_poll_ms: 1,
        convergence_timeout_ms: 60_000,
        cleanup_timeout_ms: 30_000,
        publication_timeout_ms: 30_000,
        cpu_millis: 1_000,
        memory_bytes: 512 * 1024 * 1024,
        pids: 256,
        output_max_bytes: 128 * 1024 * 1024,
    })
}

pub(super) fn workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        BUILD_WORKFLOW_NAME,
        BUILD_WORKFLOW_VERSION,
        "a3s-cloud",
        "main",
    )
}

pub(super) fn legacy_workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        BUILD_WORKFLOW_NAME,
        LEGACY_BUILD_WORKFLOW_VERSION,
        "a3s-cloud",
        "main",
    )
}

pub(super) fn artifact(digest_character: char, size_bytes: u64) -> Result<BuildArtifact, String> {
    let digest = format!("sha256:{}", digest_character.to_string().repeat(64));
    BuildArtifact::new(
        artifact_uri(&digest)?,
        digest,
        NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
        size_bytes,
    )
}

pub(super) fn revision(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    source_revision_id: SourceRevisionId,
    accepted_at: chrono::DateTime<Utc>,
) -> Result<ExternalSourceRevision, String> {
    ExternalSourceRevision::accept(NewExternalSourceRevision {
        organization_id,
        project_id,
        environment_id,
        id: source_revision_id,
        repository: GitRepository::parse(GitProvider::Github, "https://github.com/A3S-Lab/Cloud")?,
        commit_sha: GitCommitSha::parse("b".repeat(40))?,
        recipe: BuildRecipe::dockerfile(
            BuildRecipe::SCHEMA,
            BuildRecipe::DOCKERFILE_KIND,
            ".",
            "Dockerfile",
            None,
            vec!["linux/amd64".into()],
        )?,
        accepted_at,
    })
}

pub(super) async fn accept_revision(
    sources: &InMemorySourceRevisionRepository,
    revision: ExternalSourceRevision,
) -> Result<(), Box<dyn std::error::Error>> {
    let organization_id = revision.organization_id;
    sources
        .accept(AcceptSourceRevision {
            revision,
            webhook_delivery: None,
            idempotency: IdempotencyRequest::new(
                "test.build.source",
                Uuid::now_v7().to_string(),
                b"build-source",
            )?,
            event: event(organization_id),
        })
        .await?;
    Ok(())
}

pub(super) fn build_capabilities(supports_index: bool) -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: a3s_runtime::ProviderId::parse("test-build-runtime")
            .expect("valid provider ID"),
        provider_build: "test-build-runtime-1".into(),
        unit_classes: vec![RuntimeUnitClass::Task],
        artifact_media_types: vec![if supports_index {
            BUILDER_MEDIA_TYPE.into()
        } else {
            "application/vnd.oci.image.manifest.v1+json".into()
        }],
        isolation_levels: vec![IsolationLevel::Container],
        network_modes: vec![NetworkMode::None],
        mount_kinds: vec![MountKind::Artifact, MountKind::Volume],
        health_check_kinds: Vec::new(),
        resource_controls: vec![
            ResourceControl::Cpu,
            ResourceControl::Memory,
            ResourceControl::Pids,
            ResourceControl::ExecutionTimeout,
        ],
        features: vec![
            RuntimeFeature::DurableIdentity,
            RuntimeFeature::Remove,
            RuntimeFeature::OutputArtifacts,
        ],
    }
}

pub(super) async fn ready_node(
    nodes: &InMemoryNodeRepository,
    organization_id: OrganizationId,
    enrolled_at: chrono::DateTime<Utc>,
    name: &str,
    capabilities: RuntimeCapabilities,
) -> Result<(NodeId, Uuid), Box<dyn std::error::Error>> {
    capabilities.validate()?;
    let token_id = EnrollmentTokenId::new();
    let token_secret = token_id.as_uuid().simple().to_string().repeat(2);
    let secret = format!("a3sn_{token_secret}");
    let credential = EnrollmentTokenCredential::from_secret(&secret)?;
    let token = EnrollmentToken::new(
        token_id,
        organization_id,
        name,
        credential.clone(),
        enrolled_at,
        enrolled_at + Duration::minutes(5),
    )?;
    nodes
        .issue_enrollment_token(
            token,
            event(organization_id),
            IdempotencyRequest::new(
                "test.build.enrollment",
                token_id.to_string(),
                token_id.to_string().as_bytes(),
            )?,
        )
        .await?;
    let stored = NodeCapabilities::new(
        capabilities.provider_id.to_string(),
        capabilities.provider_build.clone(),
        serde_json::to_value(&capabilities)?,
    )?;
    let agent_instance_id = Uuid::now_v7();
    let reservation = nodes
        .reserve_enrollment(
            &credential,
            NodeEnrollmentDraft {
                proposed_node_id: NodeId::new(),
                name: NodeName::new(name)?,
                agent_instance_id,
                agent_version: "0.1.0".into(),
                capabilities: stored.clone(),
                request_digest: format!("sha256:{}", "c".repeat(64)),
                requested_at: enrolled_at,
            },
        )
        .await?;
    nodes
        .record_heartbeat(NodeHeartbeatUpdate {
            node_id: reservation.node.id,
            agent_instance_id,
            agent_version: "0.1.0".into(),
            capabilities: stored,
            observed_at: enrolled_at + Duration::milliseconds(1),
        })
        .await?;
    Ok((reservation.node.id, agent_instance_id))
}

pub(super) async fn lease(
    nodes: &InMemoryNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    after_sequence: u64,
) -> Result<
    a3s_cloud_contracts::NodeCommandLeaseResponse,
    crate::modules::shared_kernel::domain::RepositoryError,
> {
    let now = Utc::now();
    nodes
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence,
                max_commands: 10,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            now,
            now + Duration::seconds(1),
        )
        .await
}

pub(super) async fn record_observation(
    nodes: &InMemoryNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    capabilities: &RuntimeCapabilities,
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
    observation: RuntimeObservation,
) -> Result<(), Box<dyn std::error::Error>> {
    let observed_at = Utc::now();
    nodes
        .record_observations(
            NodeObservationBatch {
                schema: NodeObservationBatch::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                sent_at: observed_at,
                heartbeat: NodeHeartbeat {
                    schema: NodeHeartbeat::SCHEMA.into(),
                    node_id: node_id.as_uuid(),
                    agent_instance_id,
                    observed_at,
                    agent_version: "0.1.0".into(),
                    runtime_capabilities: capabilities.clone(),
                },
                observations: vec![RuntimeObservationReport {
                    report_id: Uuid::now_v7(),
                    command_id: Some(command.command_id),
                    observed_at,
                    observation,
                }],
            },
            observed_at,
        )
        .await?;
    Ok(())
}

pub(super) fn succeeded_observation(
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
    output: &BuildArtifact,
) -> Result<RuntimeObservation, String> {
    let now_ms = u64::try_from(Utc::now().timestamp_millis())
        .map_err(|_| "test clock predates Unix epoch")?;
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        spec_digest: spec.digest()?,
        class: RuntimeUnitClass::Task,
        state: RuntimeUnitState::Succeeded,
        provider_resource_id: Some("build-container-1".into()),
        provider_build: Some("test-build-runtime-1".into()),
        observed_at_ms: now_ms,
        started_at_ms: Some(now_ms.saturating_sub(1)),
        finished_at_ms: Some(now_ms),
        health: None,
        outputs: vec![RuntimeOutputArtifact {
            name: "oci-layout".into(),
            artifact: ArtifactRef {
                uri: output.uri.clone(),
                digest: output.digest.clone(),
                media_type: output.media_type.clone(),
            },
            size_bytes: output.size_bytes,
        }],
        usage: None,
        evidence: Some(a3s_runtime::contract::RuntimeEvidence {
            provider_build: "test-build-runtime-1".into(),
            spec_digest: spec.digest()?,
            semantics_profile_digest: spec.semantics_profile_digest.clone(),
            claims: BTreeMap::new(),
        }),
        provider_attestation: None,
        failure: None,
    };
    observation.validate_against(spec)?;
    Ok(observation)
}

pub(super) struct RecordingInputPreparer {
    artifact: BuildArtifact,
    prepares: AtomicUsize,
    removals: AtomicUsize,
}

impl RecordingInputPreparer {
    fn new(artifact: BuildArtifact) -> Self {
        Self {
            artifact,
            prepares: AtomicUsize::new(0),
            removals: AtomicUsize::new(0),
        }
    }

    pub(super) fn prepares(&self) -> usize {
        self.prepares.load(Ordering::SeqCst)
    }

    pub(super) fn removals(&self) -> usize {
        self.removals.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl IBuildInputPreparer for RecordingInputPreparer {
    async fn prepare(
        &self,
        build: &BuildRun,
        revision: &ExternalSourceRevision,
    ) -> Result<PreparedBuildInput, BuildInputPreparationError> {
        if build.organization_id != revision.organization_id
            || build.project_id != revision.project_id
            || build.environment_id != revision.environment_id
            || build.source_revision_id != revision.id
        {
            return Err(BuildInputPreparationError::Conflict);
        }
        self.prepares.fetch_add(1, Ordering::SeqCst);
        Ok(PreparedBuildInput {
            source_content_digest: format!("sha256:{}", "d".repeat(64)),
            artifact: self.artifact.clone(),
        })
    }

    async fn remove(&self, _build: &BuildRun) -> Result<(), BuildInputPreparationError> {
        self.removals.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

pub(super) struct RecordingOutputValidator {
    artifact: BuildArtifact,
    failure: Option<BuildOutputValidationError>,
    validations: AtomicUsize,
}

impl RecordingOutputValidator {
    fn new(artifact: BuildArtifact, failure: Option<BuildOutputValidationError>) -> Self {
        Self {
            artifact,
            failure,
            validations: AtomicUsize::new(0),
        }
    }

    pub(super) fn artifact(&self) -> &BuildArtifact {
        &self.artifact
    }

    pub(super) fn validations(&self) -> usize {
        self.validations.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl IBuildOutputValidator for RecordingOutputValidator {
    async fn validate(
        &self,
        artifact: &BuildArtifact,
        recipe: &BuildRecipe,
    ) -> Result<ValidatedOciBuildOutput, BuildOutputValidationError> {
        self.validations.fetch_add(1, Ordering::SeqCst);
        if artifact != &self.artifact {
            return Err(BuildOutputValidationError::Integrity(
                "test Runtime output changed identity".into(),
            ));
        }
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        Ok(ValidatedOciBuildOutput {
            artifact: artifact.clone(),
            descriptor: OciDescriptor::new(
                "application/vnd.oci.image.manifest.v1+json",
                format!("sha256:{}", "e".repeat(64)),
                512,
            )
            .map_err(BuildOutputValidationError::Invalid)?,
            platforms: recipe.platforms().to_vec(),
            content_bytes: 2048,
            blob_count: 3,
        })
    }
}

pub(super) struct RecordingPublisher {
    publications: AtomicUsize,
    lookups: AtomicUsize,
    published: AtomicBool,
    pause_next_publication: AtomicBool,
    publication_started: Notify,
    publication_release: Notify,
}

impl RecordingPublisher {
    fn new() -> Self {
        Self {
            publications: AtomicUsize::new(0),
            lookups: AtomicUsize::new(0),
            published: AtomicBool::new(false),
            pause_next_publication: AtomicBool::new(false),
            publication_started: Notify::new(),
            publication_release: Notify::new(),
        }
    }

    pub(super) fn publications(&self) -> usize {
        self.publications.load(Ordering::SeqCst)
    }

    pub(super) fn lookups(&self) -> usize {
        self.lookups.load(Ordering::SeqCst)
    }

    pub(super) fn pause_next_publication(&self) {
        self.pause_next_publication.store(true, Ordering::SeqCst);
    }

    pub(super) async fn wait_for_publication(&self) {
        self.publication_started.notified().await;
    }

    pub(super) fn resume_publication(&self) {
        self.publication_release.notify_one();
    }
}

#[async_trait]
impl IBuildArtifactPublisher for RecordingPublisher {
    fn target_for(
        &self,
        build: &BuildRun,
    ) -> Result<OciPublicationTarget, BuildArtifactPublicationError> {
        let output = build.output.as_ref().ok_or_else(|| {
            BuildArtifactPublicationError::Invalid("test build output is missing".into())
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
        request: &OciPublicationRequest,
    ) -> Result<Option<PublishedOciArtifact>, BuildArtifactPublicationError> {
        self.lookups.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .published
            .load(Ordering::SeqCst)
            .then(|| PublishedOciArtifact::from_target(&request.target)))
    }

    async fn publish(
        &self,
        request: &OciPublicationRequest,
    ) -> Result<PublishedOciArtifact, BuildArtifactPublicationError> {
        self.publications.fetch_add(1, Ordering::SeqCst);
        self.published.store(true, Ordering::SeqCst);
        if self.pause_next_publication.swap(false, Ordering::SeqCst) {
            self.publication_started.notify_one();
            self.publication_release.notified().await;
        }
        Ok(PublishedOciArtifact::from_target(&request.target))
    }
}

pub(super) struct FailOnceStepCompletionStore {
    inner: InMemoryEventStore,
    step_id: &'static str,
    armed: AtomicBool,
}

impl FailOnceStepCompletionStore {
    pub(super) fn new(step_id: &'static str) -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            step_id,
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for FailOnceStepCompletionStore {
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
            FlowEvent::StepCompleted { step_id, .. } if step_id == self.step_id
        );
        if is_target && self.armed.swap(false, Ordering::SeqCst) {
            return Err(FlowError::Store(format!(
                "injected loss before persisting {run_id} step {} completion",
                self.step_id
            )));
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

fn event(organization_id: OrganizationId) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: "test.build.fixture".into(),
        schema_version: 1,
        organization_id: organization_id.as_uuid(),
        aggregate_id: Uuid::now_v7(),
        aggregate_version: 1,
        occurred_at: Utc::now(),
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        payload: serde_json::json!({}),
    }
}
