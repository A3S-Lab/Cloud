use super::{
    require, BuildFlowConfig, BuildFlowConfigOptions, OperatorGate, RegistryGate, BUILDER_DIGEST,
    DEFAULT_VOLUME_ID,
};
use crate::infrastructure::{connect_and_migrate, connect_flow};
use crate::modules::artifacts::application::{BUILD_WORKFLOW_NAME, BUILD_WORKFLOW_VERSION};
use crate::modules::artifacts::domain::{
    BuildArtifact, BuildArtifactPublicationError, BuildEvidence, BuildEvidenceGenerationError,
    BuildInputPreparationError, BuildRun, IBuildArtifactPublisher, IBuildEvidenceGenerator,
    IBuildInputPreparer, IBuildRunRepository, OciPublicationRequest, OciPublicationTarget,
    PreparedBuildInput, PublishedOciArtifact, RequestBuildCancellationBundle,
    RequestBuildRetryBundle,
};
use crate::modules::artifacts::{
    LocalNodeArtifactStore, OciRegistryArtifactPublisher, OciRegistryArtifactPublisherOptions,
    PostgresBuildRunRepository, RuntimeBuildEvidenceGenerator, RuntimeBuildOutputValidator,
};
use crate::modules::fleet::domain::entities::EnrollmentToken;
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeEnrollmentDraft, NodeHeartbeatUpdate,
};
use crate::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName,
};
use crate::modules::fleet::PostgresNodeRepository;
use crate::modules::identity::domain::entities::Organization;
use crate::modules::identity::domain::repositories::IOrganizationRepository;
use crate::modules::identity::domain::value_objects::OrganizationName;
use crate::modules::identity::PostgresIdentityRepository;
use crate::modules::projects::domain::entities::{Environment, Project};
use crate::modules::projects::domain::repositories::{IEnvironmentRepository, IProjectRepository};
use crate::modules::projects::domain::value_objects::{EnvironmentName, ProjectName};
use crate::modules::projects::PostgresProjectsRepository;
use crate::modules::shared_kernel::domain::{
    BuildRunId, EnrollmentTokenId, EnvironmentId, IdempotencyRequest, IdempotentWrite, NodeId,
    OrganizationId, ProjectId, RepositoryError, SourceRevisionId,
};
use crate::modules::sources::domain::{
    AcceptSourceRevision, ExternalSourceRevision, ISourceRevisionRepository,
};
use crate::modules::sources::PostgresSourceRevisionRepository;
use a3s_cloud_contracts::{
    DomainEventEnvelope, NodeCommandAck, NodeCommandLeaseRequest, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult, NodeHeartbeat, NodeObservationBatch,
    RuntimeObservationReport,
};
use a3s_flow::{FlowEvent, FlowEventStore, PostgresEventStore, WorkflowRunStatus, WorkflowSpec};
use a3s_orm::PostgresExecutor;
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, MountKind, NetworkMode, ResourceControl, RuntimeCapabilities,
    RuntimeEvidence, RuntimeFeature, RuntimeObservation, RuntimeOutputArtifact, RuntimeRemoval,
    RuntimeUnitClass, RuntimeUnitState,
};
use a3s_runtime::ProviderId;
use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use url::Url;
use uuid::Uuid;

mod audit;
mod fixture;
mod probe;

use audit::durable_counts;
use fixture::create_fixture;
use probe::*;

const POSTGRES_ENV: &str = "A3S_CLOUD_TEST_G0_POSTGRES_URL";
const PROBE_BOUNDARY_ENV: &str = "A3S_CLOUD_TEST_G0_CRASH_BOUNDARY";
const PROBE_ROOT_ENV: &str = "A3S_CLOUD_TEST_G0_CRASH_ROOT";
const PROBE_ORGANIZATION_ENV: &str = "A3S_CLOUD_TEST_G0_CRASH_ORGANIZATION_ID";
const PROBE_BUILD_ENV: &str = "A3S_CLOUD_TEST_G0_CRASH_BUILD_ID";
const PUBLICATION_MARKER: &str = "publication-before-projection.json";
const EVIDENCE_MARKER: &str = "evidence-before-flow-completion.json";
const PROBE_TEST: &str = "modules::artifacts::infrastructure::build_flow::tests::runtime_gate::process_death::g0_signed_build_process_death_probe";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProcessDeathGateEvidence {
    publication_process_termination: &'static str,
    evidence_process_termination: &'static str,
    logical_publication_count: u32,
    verified_evidence_document_count: u32,
    publish_step_completion_count: u32,
    attest_step_completion_count: u32,
    apply_command_count: u32,
    cleanup_command_count: u32,
    cleanup_acknowledgement_count: u32,
    publication_artifact_digest: String,
    evidence_document_digest: String,
    flow_history_digest: String,
}

impl ProcessDeathGateEvidence {
    pub(super) fn validate(&self) -> Result<(), Box<dyn Error>> {
        require(
            self.publication_process_termination == "SIGKILL"
                && self.evidence_process_termination == "SIGKILL"
                && self.logical_publication_count == 1
                && self.verified_evidence_document_count == 1
                && self.publish_step_completion_count == 1
                && self.attest_step_completion_count == 1
                && self.apply_command_count == 1
                && self.cleanup_command_count == 1
                && self.cleanup_acknowledgement_count == 1
                && valid_sha256(&self.publication_artifact_digest)
                && valid_sha256(&self.evidence_document_digest)
                && valid_sha256(&self.flow_history_digest),
            "G0 process-death evidence is incomplete",
        )
    }
}

#[test]
fn process_death_evidence_requires_canonical_sha256_digests() {
    assert!(valid_sha256(&format!("sha256:{}", "a".repeat(64))));
    assert!(!valid_sha256(&format!("sha256:{}", "A".repeat(64))));
    assert!(!valid_sha256(&format!("sha256:{}", "a".repeat(63))));
}

pub(super) async fn certify(
    root: &Path,
    revision: ExternalSourceRevision,
    input_artifact: BuildArtifact,
    runtime_output: BuildArtifact,
) -> Result<ProcessDeathGateEvidence, Box<dyn Error>> {
    let postgres_url = required_environment(POSTGRES_ENV)?;
    let paths = ProbePaths::new(root);
    let executor = connect_and_migrate(&postgres_url, 8).await?;
    let fixture = create_fixture(
        &postgres_url,
        &executor,
        &paths,
        revision,
        input_artifact,
        runtime_output,
    )
    .await?;

    let publication_status = crash_at_boundary(
        CrashBoundary::Publication,
        &paths,
        &postgres_url,
        fixture.organization_id,
        fixture.build_id,
    )
    .await?;
    require_sigkill(publication_status, "publication-before-projection")?;
    let published_marker = read_marker::<PublishedOciArtifact>(&paths.publication_marker())?;
    let builds = Arc::new(PostgresBuildRunRepository::new(executor.clone()));
    let publishing = builds
        .find(fixture.organization_id, fixture.build_id)
        .await?;
    require(
        publishing.published_artifact.is_none()
            && publishing.publication_target.is_some()
            && publishing.status == crate::modules::artifacts::domain::BuildRunStatus::Publishing,
        "publication crash projected the remote artifact before process death",
    )?;
    let history_after_publication = flow_history(&postgres_url, &fixture.run_id).await?;
    require(
        completed_steps(&history_after_publication, "publish") == 0,
        "publication crash persisted the publish Flow completion",
    )?;
    verify_remote_publication(&paths, &publishing, &published_marker).await?;

    let evidence_status = crash_at_boundary(
        CrashBoundary::Evidence,
        &paths,
        &postgres_url,
        fixture.organization_id,
        fixture.build_id,
    )
    .await?;
    require_sigkill(evidence_status, "evidence-before-Flow-completion")?;
    let evidence_marker = read_marker::<BuildEvidence>(&paths.evidence_marker())?;
    evidence_marker.validate().map_err(std::io::Error::other)?;
    let attesting = builds
        .find(fixture.organization_id, fixture.build_id)
        .await?;
    require(
        attesting.published_artifact.as_ref() == Some(&published_marker)
            && attesting.evidence.as_deref() == Some(&evidence_marker)
            && attesting.status == crate::modules::artifacts::domain::BuildRunStatus::Attesting,
        "evidence crash did not preserve exactly one verified BuildRun document",
    )?;
    let history_after_evidence = flow_history(&postgres_url, &fixture.run_id).await?;
    require(
        completed_steps(&history_after_evidence, "publish") == 1
            && completed_steps(&history_after_evidence, "attest") == 0,
        "evidence crash crossed the wrong Flow completion boundary",
    )?;

    let final_runtime = runtime_with_rejecting_providers(&executor, &paths)?;
    let flow = connect_flow(&postgres_url, Arc::new(final_runtime)).await?;
    flow.engine()
        .start_with_id(
            fixture.run_id.clone(),
            workflow_spec(),
            flow_input(fixture.organization_id, fixture.build_id),
        )
        .await?;
    require(
        flow.engine().snapshot(&fixture.run_id).await?.status == WorkflowRunStatus::Suspended,
        "recovered signed build did not wait for authoritative Runtime cleanup",
    )?;

    let nodes = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let cleanup = lease_single_command(
        nodes.as_ref(),
        fixture.node_id,
        fixture.agent_instance_id,
        fixture.apply_sequence,
    )
    .await?;
    let NodeCommandPayload::RuntimeRemove { request } = &cleanup.payload else {
        return Err("G0 recovery did not dispatch a Runtime removal".into());
    };
    acknowledge_removal(nodes.as_ref(), &cleanup, request).await?;
    flow.engine()
        .resume_due_waits(Utc::now() + Duration::seconds(5))
        .await?;
    require(
        flow.engine().snapshot(&fixture.run_id).await?.status == WorkflowRunStatus::Completed,
        "G0 recovered Flow did not complete after exact cleanup evidence",
    )?;
    let completed = builds
        .find(fixture.organization_id, fixture.build_id)
        .await?;
    require(
        completed.status == crate::modules::artifacts::domain::BuildRunStatus::Succeeded
            && completed.published_artifact.as_ref() == Some(&published_marker)
            && completed.evidence.as_deref() == Some(&evidence_marker),
        "G0 recovery changed the publication or verified evidence",
    )?;
    let final_history = flow_history(&postgres_url, &fixture.run_id).await?;
    let serialized_history = serde_json::to_vec(&final_history)?;
    let operator = OperatorGate::from_environment()?
        .ok_or("G0 process-death gate requires the operator provider profile")?;
    operator.reject_protected_material(&serde_json::to_vec(&completed)?)?;
    operator.reject_protected_material(&serialized_history)?;
    operator.reject_protected_material(&serde_json::to_vec(&cleanup)?)?;

    let counts = durable_counts(
        &executor,
        fixture.organization_id,
        fixture.build_id,
        fixture.node_id,
    )
    .await?;
    let result = ProcessDeathGateEvidence {
        publication_process_termination: "SIGKILL",
        evidence_process_termination: "SIGKILL",
        logical_publication_count: counts.logical_publications,
        verified_evidence_document_count: counts.evidence_documents,
        publish_step_completion_count: completed_steps(&final_history, "publish"),
        attest_step_completion_count: completed_steps(&final_history, "attest"),
        apply_command_count: counts.apply_commands,
        cleanup_command_count: counts.cleanup_commands,
        cleanup_acknowledgement_count: counts.cleanup_acknowledgements,
        publication_artifact_digest: published_marker.digest,
        evidence_document_digest: digest_json(&evidence_marker)?,
        flow_history_digest: digest_bytes(&serialized_history),
    };
    result.validate()?;
    Ok(result)
}

pub(super) async fn run_probe() -> Result<(), Box<dyn Error>> {
    let environment = ProbeEnvironment::read()?;
    let executor = PostgresExecutor::connect_no_tls(&environment.postgres_url, 4)?;
    let registry = RegistryGate::from_environment(true)?;
    let validator = validator(&environment.paths)?;
    let publisher: Arc<dyn IBuildArtifactPublisher> = Arc::new(OciRegistryArtifactPublisher::new(
        Arc::clone(&validator),
        StdDuration::from_secs(30),
        registry.insecure_hosts.clone(),
        OciRegistryArtifactPublisherOptions {
            registry: registry.authority.clone(),
            repository_prefix: "a3s-cloud/g0-process-death".into(),
            credential_env: registry.credential.name().into(),
            allow_anonymous: false,
        },
    )?);
    let base_builds = Arc::new(PostgresBuildRunRepository::new(executor.clone()));
    let (builds, publisher): (
        Arc<dyn IBuildRunRepository>,
        Arc<dyn IBuildArtifactPublisher>,
    ) = match environment.boundary {
        CrashBoundary::Publication => (
            base_builds,
            Arc::new(CrashAfterPublication {
                inner: publisher,
                marker: environment.paths.publication_marker(),
            }),
        ),
        CrashBoundary::Evidence => (
            Arc::new(CrashAfterEvidenceSave {
                inner: base_builds,
                marker: environment.paths.evidence_marker(),
            }),
            publisher,
        ),
    };
    let operator = OperatorGate::from_environment()?
        .ok_or("G0 process-death probe requires the operator provider profile")?;
    let evidence: Arc<dyn IBuildEvidenceGenerator> = Arc::new(RuntimeBuildEvidenceGenerator::new(
        validator,
        Arc::new(operator.signer()?),
        builder(),
    )?);
    let runtime = runtime(
        &executor,
        &environment.paths,
        builds,
        publisher,
        evidence,
        Arc::new(NoopInputPreparer),
    )?;
    let flow = connect_flow(&environment.postgres_url, Arc::new(runtime)).await?;
    flow.engine()
        .start_with_id(
            environment.build_id.to_string(),
            workflow_spec(),
            flow_input(environment.organization_id, environment.build_id),
        )
        .await?;
    for _ in 0..64 {
        if flow
            .engine()
            .resume_due_waits(Utc::now() + Duration::minutes(5))
            .await?
            .is_empty()
        {
            break;
        }
    }
    let build = PostgresBuildRunRepository::new(executor)
        .find(environment.organization_id, environment.build_id)
        .await?;
    let snapshot = flow
        .engine()
        .snapshot(&environment.build_id.to_string())
        .await?;
    let history =
        flow_history(&environment.postgres_url, &environment.build_id.to_string()).await?;
    Err(format!(
        "G0 process-death probe returned before its {} boundary: build={}, failure={:?}, flow={:?}, events={}, prepare={}, dispatch={}, validate={}, publication-target={}, publish={}, attest={}, last={:?}",
        environment.boundary.as_str(),
        build.status.as_str(),
        build.failure,
        snapshot.status,
        history.len(),
        completed_steps(&history, "prepare"),
        completed_steps(&history, "dispatch"),
        completed_steps(&history, "validate"),
        completed_steps(&history, "publication-target"),
        completed_steps(&history, "publish"),
        completed_steps(&history, "attest"),
        history.last().map(|event| &event.event),
    )
    .into())
}

#[tokio::test]
#[ignore = "private subprocess used only by the G0 signed-build process-death gate"]
async fn g0_signed_build_process_death_probe() {
    run_probe()
        .await
        .expect("run G0 signed-build process-death probe");
}

fn runtime(
    executor: &PostgresExecutor,
    paths: &ProbePaths,
    builds: Arc<dyn IBuildRunRepository>,
    publisher: Arc<dyn IBuildArtifactPublisher>,
    evidence: Arc<dyn IBuildEvidenceGenerator>,
    inputs: Arc<dyn IBuildInputPreparer>,
) -> Result<super::super::super::BuildFlowRuntime, Box<dyn Error>> {
    let sources: Arc<dyn ISourceRevisionRepository> =
        Arc::new(PostgresSourceRevisionRepository::new(executor.clone()));
    let nodes = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let node_repository: Arc<dyn INodeRepository> = nodes.clone();
    let node_control: Arc<dyn INodeControlRepository> = nodes;
    Ok(super::super::super::BuildFlowRuntime::new(
        super::super::super::BuildFlowRuntimeDependencies {
            builds,
            sources,
            inputs,
            outputs: validator(paths)?,
            publisher,
            evidence,
            nodes: node_repository,
            node_control,
        },
        flow_config()?,
    ))
}

fn runtime_with_rejecting_providers(
    executor: &PostgresExecutor,
    paths: &ProbePaths,
) -> Result<super::super::super::BuildFlowRuntime, Box<dyn Error>> {
    runtime(
        executor,
        paths,
        Arc::new(PostgresBuildRunRepository::new(executor.clone())),
        Arc::new(RejectingPublisher),
        Arc::new(RejectingEvidenceGenerator),
        Arc::new(NoopInputPreparer),
    )
}

fn validator(paths: &ProbePaths) -> Result<Arc<RuntimeBuildOutputValidator>, Box<dyn Error>> {
    let artifacts = Arc::new(LocalNodeArtifactStore::new(
        &paths.artifact_store,
        1024 * 1024 * 1024,
    )?);
    Ok(Arc::new(RuntimeBuildOutputValidator::new(
        artifacts,
        &paths.validation_root,
        512 * 1024 * 1024,
        100_000,
        1024 * 1024 * 1024,
        10_000,
        1024 * 1024 * 1024,
    )?))
}

async fn verify_remote_publication(
    paths: &ProbePaths,
    build: &BuildRun,
    expected: &PublishedOciArtifact,
) -> Result<(), Box<dyn Error>> {
    let registry = RegistryGate::from_environment(true)?;
    let publisher = OciRegistryArtifactPublisher::new(
        validator(paths)?,
        StdDuration::from_secs(30),
        registry.insecure_hosts.clone(),
        OciRegistryArtifactPublisherOptions {
            registry: registry.authority.clone(),
            repository_prefix: "a3s-cloud/g0-process-death".into(),
            credential_env: registry.credential.name().into(),
            allow_anonymous: false,
        },
    )?;
    let request = OciPublicationRequest::new(
        build
            .publication_target
            .clone()
            .ok_or("publishing BuildRun omitted its publication target")?,
        build
            .output
            .clone()
            .ok_or("publishing BuildRun omitted its validated output")?,
    )?;
    require(
        publisher.find(&request).await?.as_ref() == Some(expected),
        "G0 publication crash did not leave one complete remote OCI graph",
    )
}

async fn ready_node(
    nodes: &PostgresNodeRepository,
    organization_id: OrganizationId,
    enrolled_at: chrono::DateTime<Utc>,
    capabilities: RuntimeCapabilities,
) -> Result<(NodeId, Uuid), Box<dyn Error>> {
    capabilities.validate()?;
    let token_id = EnrollmentTokenId::new();
    let token_secret = token_id.as_uuid().simple().to_string().repeat(2);
    let credential = EnrollmentTokenCredential::from_secret(&format!("a3sn_{token_secret}"))?;
    nodes
        .issue_enrollment_token(
            EnrollmentToken::new(
                token_id,
                organization_id,
                "g0-process-death-node",
                credential.clone(),
                enrolled_at,
                enrolled_at + Duration::minutes(10),
            )?,
            event(
                "fleet.enrollment.issued",
                organization_id,
                token_id.as_uuid(),
            ),
            idempotency("g0.enrollment", token_id.as_uuid())?,
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
                name: NodeName::new("g0-process-death-node")?,
                agent_instance_id,
                agent_version: "g0-gate".into(),
                capabilities: stored.clone(),
                request_digest: digest_bytes(token_id.to_string().as_bytes()),
                requested_at: enrolled_at,
            },
        )
        .await?;
    nodes
        .record_heartbeat(NodeHeartbeatUpdate {
            node_id: reservation.node.id,
            agent_instance_id,
            agent_version: "g0-gate".into(),
            capabilities: stored,
            observed_at: enrolled_at + Duration::milliseconds(1),
        })
        .await?;
    Ok((reservation.node.id, agent_instance_id))
}

fn build_capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: ProviderId::parse("g0-process-death-runtime").expect("provider ID"),
        provider_build: "g0-process-death-runtime-1".into(),
        unit_classes: vec![RuntimeUnitClass::Task],
        artifact_media_types: vec!["application/vnd.oci.image.index.v1+json".into()],
        isolation_levels: vec![IsolationLevel::Container],
        network_modes: vec![NetworkMode::None],
        mount_kinds: vec![MountKind::Artifact, MountKind::Volume, MountKind::Tmpfs],
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

async fn lease_single_command(
    nodes: &PostgresNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    after_sequence: u64,
) -> Result<a3s_cloud_contracts::NodeCommandEnvelope, Box<dyn Error>> {
    let now = Utc::now();
    let lease = nodes
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
            now + Duration::minutes(2),
        )
        .await?;
    require(
        lease.commands.len() == 1,
        format!(
            "G0 process-death gate expected one node command, found {}",
            lease.commands.len()
        ),
    )?;
    lease
        .commands
        .into_iter()
        .next()
        .ok_or_else(|| "G0 process-death node command disappeared".into())
}

fn succeeded_observation(
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
    output: &BuildArtifact,
) -> Result<RuntimeObservation, Box<dyn Error>> {
    let now_ms = u64::try_from(Utc::now().timestamp_millis())?;
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        spec_digest: spec.digest()?,
        class: RuntimeUnitClass::Task,
        state: RuntimeUnitState::Succeeded,
        provider_resource_id: Some("g0-process-death-synthetic-runtime".into()),
        provider_build: Some("g0-process-death-runtime-1".into()),
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
        evidence: Some(RuntimeEvidence {
            provider_build: "g0-process-death-runtime-1".into(),
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

async fn record_observation(
    nodes: &PostgresNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    capabilities: RuntimeCapabilities,
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
    observation: RuntimeObservation,
) -> Result<(), Box<dyn Error>> {
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
                    agent_version: "g0-gate".into(),
                    runtime_capabilities: capabilities,
                },
                observations: vec![RuntimeObservationReport {
                    report_id: Uuid::now_v7(),
                    command_id: Some(command.command_id),
                    observed_at,
                    observation,
                }],
            }
            .into(),
            observed_at,
        )
        .await?;
    Ok(())
}

async fn acknowledge_apply(
    nodes: &PostgresNodeRepository,
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
    observation: RuntimeObservation,
) -> Result<(), Box<dyn Error>> {
    let completed_at = Utc::now();
    nodes
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: command.command_id,
                lease_id: command.lease_id,
                node_id: command.node_id,
                sequence: command.sequence,
                payload_digest: command.payload_digest.clone(),
                completed_at,
                outcome: NodeCommandOutcome::Succeeded {
                    result: Box::new(NodeCommandResult::RuntimeApplied {
                        observation: Box::new(observation),
                    }),
                },
            },
            completed_at,
        )
        .await?;
    Ok(())
}

async fn acknowledge_removal(
    nodes: &PostgresNodeRepository,
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
    request: &a3s_runtime::contract::RuntimeActionRequest,
) -> Result<(), Box<dyn Error>> {
    let completed_at = Utc::now();
    nodes
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: command.command_id,
                lease_id: command.lease_id,
                node_id: command.node_id,
                sequence: command.sequence,
                payload_digest: command.payload_digest.clone(),
                completed_at,
                outcome: NodeCommandOutcome::Succeeded {
                    result: Box::new(NodeCommandResult::RuntimeRemoved {
                        removal: RuntimeRemoval {
                            schema: RuntimeRemoval::SCHEMA.into(),
                            request_id: request.request_id.clone(),
                            unit_id: request.unit_id.clone(),
                            generation: request.generation,
                            removed_at_ms: u64::try_from(completed_at.timestamp_millis())?,
                            already_absent: true,
                        },
                    }),
                },
            },
            completed_at,
        )
        .await?;
    Ok(())
}

async fn flow_history(
    postgres_url: &str,
    run_id: &str,
) -> Result<Vec<a3s_flow::FlowEventEnvelope>, Box<dyn Error>> {
    let mut url = Url::parse(postgres_url)?;
    require(
        !url.query_pairs().any(|(key, _)| key == "options"),
        "G0 PostgreSQL URL cannot override the Flow search path",
    )?;
    url.query_pairs_mut()
        .append_pair("options", "-csearch_path=a3s_flow");
    let store = PostgresEventStore::connect(url.as_str()).await?;
    Ok(store.list(run_id).await?)
}

fn completed_steps(history: &[a3s_flow::FlowEventEnvelope], step: &str) -> u32 {
    u32::try_from(
        history
            .iter()
            .filter(|event| {
                matches!(
                    &event.event,
                    FlowEvent::StepCompleted { step_id, .. } if step_id == step
                )
            })
            .count(),
    )
    .unwrap_or(u32::MAX)
}

fn flow_config() -> Result<BuildFlowConfig, String> {
    BuildFlowConfig::new(BuildFlowConfigOptions {
        builder: builder(),
        buildkit_socket_volume_id: DEFAULT_VOLUME_ID.into(),
        heartbeat_timeout_ms: 300_000,
        command_ttl_ms: 1_800_000,
        execution_timeout_ms: 1_200_000,
        observation_poll_ms: 100,
        convergence_timeout_ms: 1_800_000,
        cleanup_timeout_ms: 600_000,
        publication_timeout_ms: 600_000,
        cpu_millis: 2_000,
        memory_bytes: 1024 * 1024 * 1024,
        pids: 512,
        output_max_bytes: 512 * 1024 * 1024,
    })
}

fn builder() -> ArtifactRef {
    ArtifactRef {
        uri: format!("oci://docker.io/moby/buildkit@{BUILDER_DIGEST}"),
        digest: BUILDER_DIGEST.into(),
        media_type: "application/vnd.oci.image.index.v1+json".into(),
    }
}

fn workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        BUILD_WORKFLOW_NAME,
        BUILD_WORKFLOW_VERSION,
        "a3s-cloud",
        "main",
    )
}

fn flow_input(organization_id: OrganizationId, build_id: BuildRunId) -> serde_json::Value {
    serde_json::json!({
        "organizationId": organization_id,
        "buildRunId": build_id,
    })
}

fn idempotency(scope: &str, identity: Uuid) -> Result<IdempotencyRequest, String> {
    IdempotencyRequest::new(scope, identity.to_string(), identity.as_bytes())
}

fn event(
    event_key: &str,
    organization_id: OrganizationId,
    aggregate_id: Uuid,
) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: event_key.into(),
        schema_version: 1,
        organization_id: organization_id.as_uuid(),
        aggregate_id,
        aggregate_version: 1,
        occurred_at: Utc::now(),
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        payload: serde_json::json!({}),
    }
}

fn write_durable_json(path: &Path, value: &impl Serialize) -> Result<(), std::io::Error> {
    let body = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(&body)?;
    file.sync_all()?;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("G0 crash marker has no parent"))?;
    std::fs::File::open(parent)?.sync_all()
}

fn read_marker<T: DeserializeOwned>(path: &Path) -> Result<T, Box<dyn Error>> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn park_until_killed() -> ! {
    loop {
        std::thread::park();
    }
}

fn digest_json(value: &impl Serialize) -> Result<String, serde_json::Error> {
    serde_json::to_vec(value).map(|bytes| digest_bytes(&bytes))
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn valid_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}

fn required_environment(name: &str) -> Result<String, std::io::Error> {
    std::env::var(name).map_err(|_| std::io::Error::other(format!("G0 crash probe omitted {name}")))
}
