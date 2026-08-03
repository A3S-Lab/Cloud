use super::super::{
    BuildFlowConfig, BuildFlowConfigOptions, BuildFlowRuntime, BuildFlowRuntimeDependencies,
};
use crate::modules::artifacts::application::{BUILD_WORKFLOW_NAME, BUILD_WORKFLOW_VERSION};
use crate::modules::artifacts::domain::{
    BuildArtifact, BuildArtifactPublicationError, BuildEvidence, BuildEvidenceGenerationError,
    BuildInputPreparationError, BuildOutputValidationError, BuildRun, BuildSource,
    BuildSourceResolutionError, IBuildArtifactPublisher, IBuildEvidenceGenerator,
    IBuildInputPreparer, IBuildOutputValidator, IBuildRunRepository, IBuildSourceResolver,
    OciDescriptor, OciPublicationRequest, OciPublicationTarget, PreparedBuildInput,
    PublishedOciArtifact, ValidatedOciBuildOutput,
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
use a3s_box_runtime::BoxBuildPlan;
use a3s_cloud_contracts::{
    artifact_uri, DomainEventEnvelope, NodeBoxBuildCacheOutput, NodeBoxBuildCacheReceipt,
    NodeBoxBuildDescriptor, NodeBoxBuildOutput, NodeBoxBuildPlatform, NodeBoxBuildRequest,
    NodeCommandLeaseRequest, BOX_BUILD_OUTPUT_NAME, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_flow::{
    FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, InMemoryEventStore, WorkflowSpec,
};
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, MountKind, NetworkMode, ResourceControl, RuntimeCapabilities,
    RuntimeFeature, RuntimeOutputArtifact, RuntimeUnitClass,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

pub(super) struct BuildFixture {
    pub organization_id: OrganizationId,
    pub build: BuildRun,
    pub builds: Arc<InMemoryBuildRunRepository>,
    pub nodes: Arc<InMemoryNodeRepository>,
    pub inputs: Arc<RecordingInputPreparer>,
    pub outputs: Arc<RecordingOutputValidator>,
    pub publisher: Arc<RecordingPublisher>,
    pub evidence: Arc<RecordingEvidenceGenerator>,
    pub node_id: NodeId,
    pub agent_instance_id: Uuid,
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
        accept_revision(&sources, revision).await?;
        let build_sources = Arc::new(FixtureBuildSourceResolver {
            sources: Arc::clone(&sources),
        });
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
        ready_node(
            &nodes,
            organization_id,
            base,
            "non-box-node",
            build_capabilities("another-provider"),
        )
        .await?;
        let (node_id, agent_instance_id) = ready_node(
            &nodes,
            organization_id,
            base,
            "box-build-node",
            build_capabilities("a3s-box"),
        )
        .await?;
        let inputs = Arc::new(RecordingInputPreparer::new(artifact('1', 4096)?));
        let outputs = Arc::new(RecordingOutputValidator::new(
            artifact('2', 8192)?,
            output_failure,
        ));
        let publisher = Arc::new(RecordingPublisher::new());
        let evidence = Arc::new(RecordingEvidenceGenerator::new());
        let runtime = BuildFlowRuntime::new(
            BuildFlowRuntimeDependencies {
                builds: builds.clone(),
                sources: build_sources,
                inputs: inputs.clone(),
                outputs: outputs.clone(),
                publisher: publisher.clone(),
                evidence: evidence.clone(),
                nodes: nodes.clone(),
                node_control: nodes.clone(),
            },
            config()?,
        );
        Ok(Self {
            organization_id,
            build,
            builds,
            nodes,
            inputs,
            outputs,
            publisher,
            evidence,
            node_id,
            agent_instance_id,
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

struct FixtureBuildSourceResolver {
    sources: Arc<InMemorySourceRevisionRepository>,
}

#[async_trait]
impl IBuildSourceResolver for FixtureBuildSourceResolver {
    async fn resolve(&self, build: &BuildRun) -> Result<BuildSource, BuildSourceResolutionError> {
        let source_revision_id = build.source_revision_id().ok_or_else(|| {
            BuildSourceResolutionError::Invalid(
                "external build fixture requires a source revision subject".into(),
            )
        })?;
        let revision = self
            .sources
            .find(build.organization_id, source_revision_id)
            .await
            .map_err(|error| BuildSourceResolutionError::Storage(error.to_string()))?;
        BuildSource::from_external_revision(&revision)
            .map_err(BuildSourceResolutionError::Integrity)
    }
}

pub(super) fn config() -> Result<BuildFlowConfig, String> {
    BuildFlowConfig::new(BuildFlowConfigOptions {
        heartbeat_timeout_ms: 5_000,
        command_ttl_ms: 30_000,
        execution_timeout_ms: 10_000,
        observation_poll_ms: 1,
        convergence_timeout_ms: 60_000,
        cleanup_timeout_ms: 30_000,
        publication_timeout_ms: 30_000,
        output_max_bytes: 128 * 1024 * 1024,
        cache_max_bytes: 128 * 1024 * 1024,
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

pub(super) fn digest(fill: char) -> String {
    format!("sha256:{}", fill.to_string().repeat(64))
}

pub(super) fn artifact(fill: char, size_bytes: u64) -> Result<BuildArtifact, String> {
    let digest = digest(fill);
    BuildArtifact::new(
        artifact_uri(&digest)?,
        digest,
        NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
        size_bytes,
    )
}

pub(super) fn box_output_for(
    request: &NodeBoxBuildRequest,
    output: BuildArtifact,
) -> Result<NodeBoxBuildOutput, String> {
    request.validate()?;
    let artifact = ArtifactRef {
        uri: output.uri,
        digest: output.digest,
        media_type: output.media_type,
    };
    let descriptor = NodeBoxBuildDescriptor {
        media_type: if request.plans.len() == 1 {
            OCI_MANIFEST.into()
        } else {
            "application/vnd.oci.image.index.v1+json".into()
        },
        digest: digest('e'),
        size: 512,
    };
    let platforms = request
        .plans
        .iter()
        .map(|plan| {
            let parsed =
                BoxBuildPlan::parse_acl(&plan.plan_acl).map_err(|error| error.to_string())?;
            let identity = parsed.platform().to_string();
            let (os, architecture) = identity
                .split_once('/')
                .ok_or_else(|| "Box fixture platform is invalid".to_owned())?;
            Ok(NodeBoxBuildPlatform {
                os: os.into(),
                architecture: architecture.into(),
                variant: None,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let caches = request
        .plans
        .iter()
        .zip(&platforms)
        .map(|(plan, platform)| {
            let plan_digest = BoxBuildPlan::parse_acl(&plan.plan_acl)
                .map_err(|error| error.to_string())?
                .canonical_digest()
                .map_err(|error| error.to_string())?;
            Ok(NodeBoxBuildCacheOutput {
                operation_id: plan.operation_id.clone(),
                artifact: RuntimeOutputArtifact {
                    name: plan.cache_output_name(),
                    artifact: artifact.clone(),
                    size_bytes: 4096,
                },
                receipt: NodeBoxBuildCacheReceipt {
                    schema: NodeBoxBuildCacheReceipt::SCHEMA.into(),
                    key: digest('3'),
                    source_digest: request.source.digest.clone(),
                    plan_digest,
                    descriptor: descriptor.clone(),
                    platform: platform.clone(),
                    content_bytes: 2048,
                    entry_count: 3,
                    blob_count: 3,
                    blob_inventory_digest: digest('4'),
                },
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let output = NodeBoxBuildOutput {
        artifact: RuntimeOutputArtifact {
            name: BOX_BUILD_OUTPUT_NAME.into(),
            artifact,
            size_bytes: 8192,
        },
        descriptor,
        platforms,
        manifest_count: request.plans.len() as u64,
        content_bytes: 2048,
        blob_count: 3,
        blob_inventory_digest: digest('5'),
        caches,
    };
    output.validate()?;
    Ok(output)
}

fn revision(
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
        commit_sha: GitCommitSha::parse("a".repeat(40))?,
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

async fn accept_revision(
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

fn build_capabilities(provider_id: &str) -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: a3s_runtime::ProviderId::parse(provider_id).expect("valid provider ID"),
        provider_build: format!("{provider_id}-test"),
        unit_classes: vec![RuntimeUnitClass::Task],
        artifact_media_types: vec!["application/vnd.oci.image.index.v1+json".into()],
        isolation_levels: vec![IsolationLevel::Container],
        network_modes: vec![NetworkMode::None],
        mount_kinds: vec![MountKind::Artifact],
        health_check_kinds: Vec::new(),
        resource_controls: vec![ResourceControl::ExecutionTimeout],
        features: vec![RuntimeFeature::DurableIdentity],
    }
}

async fn ready_node(
    nodes: &InMemoryNodeRepository,
    organization_id: OrganizationId,
    enrolled_at: chrono::DateTime<Utc>,
    name: &str,
    capabilities: RuntimeCapabilities,
) -> Result<(NodeId, Uuid), Box<dyn std::error::Error>> {
    capabilities.validate()?;
    let token_id = EnrollmentTokenId::new();
    let secret = format!("a3sn_{}", token_id.as_uuid().simple().to_string().repeat(2));
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
                request_digest: digest('c'),
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
            now + Duration::seconds(5),
        )
        .await
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
        source: &BuildSource,
    ) -> Result<PreparedBuildInput, BuildInputPreparationError> {
        if build.organization_id != source.organization_id || build.subject != source.subject {
            return Err(BuildInputPreparationError::Conflict);
        }
        self.prepares.fetch_add(1, Ordering::SeqCst);
        Ok(PreparedBuildInput {
            source_content_digest: digest('d'),
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

    pub(super) fn artifact(&self) -> BuildArtifact {
        self.artifact.clone()
    }

    pub(super) fn validations(&self) -> usize {
        self.validations.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl IBuildOutputValidator for RecordingOutputValidator {
    async fn validate(
        &self,
        output: &NodeBoxBuildOutput,
        recipe: &BuildRecipe,
    ) -> Result<ValidatedOciBuildOutput, BuildOutputValidationError> {
        self.validations.fetch_add(1, Ordering::SeqCst);
        output
            .validate()
            .map_err(BuildOutputValidationError::Integrity)?;
        if output.artifact.artifact.uri != self.artifact.uri
            || output.artifact.artifact.digest != self.artifact.digest
            || output.artifact.artifact.media_type != self.artifact.media_type
            || output.artifact.size_bytes != self.artifact.size_bytes
        {
            return Err(BuildOutputValidationError::Integrity(
                "test Box output changed identity".into(),
            ));
        }
        if let Some(error) = &self.failure {
            return Err(error.clone());
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

pub(super) struct RecordingPublisher {
    publications: AtomicUsize,
}

impl RecordingPublisher {
    fn new() -> Self {
        Self {
            publications: AtomicUsize::new(0),
        }
    }

    pub(super) fn publications(&self) -> usize {
        self.publications.load(Ordering::SeqCst)
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
        _request: &OciPublicationRequest,
    ) -> Result<Option<PublishedOciArtifact>, BuildArtifactPublicationError> {
        Ok(None)
    }

    async fn publish(
        &self,
        request: &OciPublicationRequest,
    ) -> Result<PublishedOciArtifact, BuildArtifactPublicationError> {
        self.publications.fetch_add(1, Ordering::SeqCst);
        Ok(PublishedOciArtifact::from_target(&request.target))
    }
}

pub(super) struct RecordingEvidenceGenerator {
    generations: AtomicUsize,
}

impl RecordingEvidenceGenerator {
    fn new() -> Self {
        Self {
            generations: AtomicUsize::new(0),
        }
    }

    pub(super) fn generations(&self) -> usize {
        self.generations.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl IBuildEvidenceGenerator for RecordingEvidenceGenerator {
    async fn generate(
        &self,
        build: &BuildRun,
        source: &BuildSource,
        attested_at: chrono::DateTime<Utc>,
    ) -> Result<BuildEvidence, BuildEvidenceGenerationError> {
        self.generations.fetch_add(1, Ordering::SeqCst);
        if build.organization_id != source.organization_id || build.subject != source.subject {
            return Err(BuildEvidenceGenerationError::Invalid(
                "test evidence source changed identity".into(),
            ));
        }
        Ok(crate::modules::artifacts::domain::test_support::evidence_for(build, attested_at))
    }
}

pub(super) struct FailStepCompletionStore {
    inner: InMemoryEventStore,
    step_ids: Mutex<VecDeque<&'static str>>,
}

impl FailStepCompletionStore {
    pub(super) fn new(step_id: &'static str) -> Self {
        Self::sequence([step_id])
    }

    pub(super) fn sequence(step_ids: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            step_ids: Mutex::new(step_ids.into_iter().collect()),
        }
    }

    pub(super) fn remaining(&self) -> Result<Vec<&'static str>, FlowError> {
        self.step_ids
            .lock()
            .map(|step_ids| step_ids.iter().copied().collect())
            .map_err(|_| FlowError::Store("fault-injection step lock was poisoned".into()))
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
        let completed_step = match &event {
            FlowEvent::StepCompleted { step_id, .. } => Some(step_id.as_str()),
            _ => None,
        };
        if let Some(completed_step) = completed_step {
            let mut step_ids = self
                .step_ids
                .lock()
                .map_err(|_| FlowError::Store("fault-injection step lock was poisoned".into()))?;
            if step_ids.front().copied() == Some(completed_step) {
                let step_id = step_ids
                    .pop_front()
                    .ok_or_else(|| FlowError::Store("fault-injection step disappeared".into()))?;
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
