use super::runtime;
use a3s_cloud_contracts::{
    DomainEventEnvelope, NodeBoxBuildCacheOutput, NodeBoxBuildCacheReceipt, NodeBoxBuildDescriptor,
    NodeBoxBuildOutput, NodeBoxBuildPlatform, NodeBoxBuildRequest, NodeCommandAck,
    NodeCommandEnvelope, NodeCommandLeaseRequest, NodeCommandOutcome, NodeCommandPayload,
    NodeCommandResult, BOX_BUILD_OUTPUT_NAME,
};
use a3s_cloud_control_plane::modules::artifacts::{
    BuildArtifact, BuildCandidate, BuildCandidateEvidence, BuildRun, BuildSubject,
    IBuildCandidateProjectionPort, IBuildRunRepository, PostgresBuildRunRepository,
};
use a3s_cloud_control_plane::modules::fleet::domain::entities::{EnrollmentToken, NodeCommand};
use a3s_cloud_control_plane::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeEnrollmentDraft, NodeHeartbeatUpdate,
};
use a3s_cloud_control_plane::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName,
};
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    EnrollmentTokenId, EnvironmentId, IdempotencyRequest, NodeCommandId, NodeId, OrganizationId,
    ProjectId, Sha256Digest, SourceRevisionId,
};
use a3s_cloud_control_plane::modules::sources::domain::{
    AcceptSourceRevision, BuildRecipe, ExternalSourceRevision, GitCommitSha, GitProvider,
    GitRepository, ISourceRevisionRepository, NewExternalSourceRevision, SourceRevisionAccepted,
};
use a3s_cloud_control_plane::modules::sources::PostgresSourceRevisionRepository;
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, MountKind, NetworkMode, ResourceControl, RuntimeCapabilities,
    RuntimeFeature, RuntimeOutputArtifact, RuntimeUnitClass,
};
use chrono::{Duration, Utc};
use std::path::{Path, PathBuf};
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(super) struct Fixture {
    pub(super) postgres_url: String,
    pub(super) state_dir: PathBuf,
    pub(super) executor: PostgresExecutor,
    pub(super) builds: PostgresBuildRunRepository,
    pub(super) nodes: PostgresNodeRepository,
    pub(super) organization_id: OrganizationId,
    pub(super) build: BuildRun,
    pub(super) node_id: NodeId,
    pub(super) agent_instance_id: Uuid,
}

pub(super) async fn setup_fixture(
    executor: PostgresExecutor,
    postgres_url: String,
    state_dir: &Path,
) -> TestResult<Fixture> {
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let source_revision_id = SourceRevisionId::new();
    let base = Utc::now() - Duration::seconds(1);
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'Build crash tenant', 'build-crash-tenant', 1, ")
            .bind(base)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(project_id.as_uuid())
            .append(", 'Build crash project', 'build-crash-project', 1, ")
            .bind(base)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into environments (organization_id, project_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(project_id.as_uuid())
            .append(", ")
            .bind(environment_id.as_uuid())
            .append(", 'Build crash environment', 'build-crash-environment', 1, ")
            .bind(base)
            .append(")"),
        )
        .await?;

    let revision = ExternalSourceRevision::accept(NewExternalSourceRevision {
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
        accepted_at: base,
    })?;
    let repository_identity = revision.repository.identity().to_owned();
    let commit_sha = revision.commit_sha.clone();
    let recipe_digest = Sha256Digest::parse(revision.recipe_digest.clone())?;
    let source_accepted_event = SourceRevisionAccepted::envelope(&revision, Uuid::now_v7())?;
    PostgresSourceRevisionRepository::new(executor.clone())
        .accept(AcceptSourceRevision {
            revision,
            webhook_delivery: None,
            idempotency: IdempotencyRequest::new(
                "test.build.process-death.source",
                source_revision_id.to_string(),
                source_revision_id.as_uuid().as_bytes(),
            )?,
            event: source_accepted_event,
        })
        .await?;
    let builds = PostgresBuildRunRepository::new(executor.clone());
    builds
        .project_candidate(BuildCandidate::new(
            organization_id,
            BuildSubject::external_source_revision(project_id, environment_id, source_revision_id),
            BuildCandidateEvidence::external_source_revision(
                repository_identity,
                commit_sha,
                recipe_digest,
            )?,
            base,
        )?)
        .await?;
    let build = builds
        .reserve_pending(1)
        .await?
        .pop()
        .ok_or("PostgreSQL did not reserve the BuildRun fixture")?;
    let nodes = PostgresNodeRepository::new(executor.clone());
    let (node_id, agent_instance_id) = ready_node(&nodes, organization_id, base).await?;
    Ok(Fixture {
        postgres_url,
        state_dir: state_dir.to_path_buf(),
        executor,
        builds,
        nodes,
        organization_id,
        build,
        node_id,
        agent_instance_id,
    })
}

async fn ready_node(
    nodes: &PostgresNodeRepository,
    organization_id: OrganizationId,
    enrolled_at: chrono::DateTime<Utc>,
) -> TestResult<(NodeId, Uuid)> {
    let capabilities = runtime_capabilities();
    capabilities.validate()?;
    let token_id = EnrollmentTokenId::new();
    let secret = format!("a3sn_{}", token_id.as_uuid().simple().to_string().repeat(2));
    let credential = EnrollmentTokenCredential::from_secret(&secret)?;
    let token = EnrollmentToken::new(
        token_id,
        organization_id,
        "postgres-build-flow-node",
        credential.clone(),
        enrolled_at,
        enrolled_at + Duration::minutes(5),
    )?;
    nodes
        .issue_enrollment_token(
            token,
            event(organization_id, "test.build.node.enrollment-issued"),
            IdempotencyRequest::new(
                "test.build.process-death.enrollment",
                token_id.to_string(),
                token_id.as_uuid().as_bytes(),
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
                name: NodeName::new("postgres-build-flow-node")?,
                agent_instance_id,
                agent_version: "0.1.0-test".into(),
                capabilities: stored.clone(),
                request_digest: runtime::digest('c'),
                requested_at: enrolled_at,
            },
        )
        .await?;
    nodes
        .record_heartbeat(NodeHeartbeatUpdate {
            node_id: reservation.node.id,
            agent_instance_id,
            agent_version: "0.1.0-test".into(),
            capabilities: stored,
            observed_at: enrolled_at + Duration::milliseconds(1),
        })
        .await?;
    Ok((reservation.node.id, agent_instance_id))
}

fn runtime_capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: a3s_runtime::ProviderId::parse("a3s-box").expect("valid provider ID"),
        provider_build: "a3s-box-postgres-process-death".into(),
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

pub(super) async fn verify_command_count(fixture: &Fixture, expected: i64) -> TestResult {
    let connection = fixture.executor.pool().get().await?;
    let row = connection
        .query_one(
            "select count(*) from node_commands where node_id = $1",
            &[&fixture.node_id.as_uuid()],
        )
        .await?;
    let actual: i64 = row.get(0);
    if actual != expected {
        return Err(format!(
            "Fleet stored {actual} commands for the BuildRun, expected {expected}"
        )
        .into());
    }
    Ok(())
}

pub(super) async fn cleanup_command_id(
    fixture: &Fixture,
    label: &str,
) -> TestResult<NodeCommandId> {
    fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?
        .cleanup_command_id
        .ok_or_else(|| format!("{label} identity was not committed before process death").into())
}

pub(super) async fn find_command(
    fixture: &Fixture,
    command_id: NodeCommandId,
    label: &str,
) -> TestResult<NodeCommand> {
    fixture
        .nodes
        .find_command(fixture.node_id, command_id)
        .await?
        .ok_or_else(|| format!("{label} was not durably queued").into())
}

pub(super) fn require_same_command(
    before: &NodeCommand,
    after: &NodeCommand,
    label: &str,
) -> TestResult {
    if before != after {
        return Err(format!("{label} changed across persistent Flow replay").into());
    }
    Ok(())
}

pub(super) async fn lease_one(
    fixture: &Fixture,
    after_sequence: u64,
) -> TestResult<NodeCommandEnvelope> {
    let now = Utc::now();
    let response = fixture
        .nodes
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: fixture.node_id.as_uuid(),
                agent_instance_id: fixture.agent_instance_id,
                after_sequence,
                max_commands: 10,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            now,
            now + Duration::minutes(10),
        )
        .await?;
    if response.commands.len() != 1 {
        return Err(format!(
            "expected one Box command after sequence {after_sequence}, got {}",
            response.commands.len()
        )
        .into());
    }
    Ok(response.commands.into_iter().next().expect("one command"))
}

pub(super) async fn acknowledge(
    fixture: &Fixture,
    command: &NodeCommandEnvelope,
    result: NodeCommandResult,
) -> TestResult {
    fixture
        .nodes
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: command.command_id,
                lease_id: command.lease_id,
                node_id: command.node_id,
                sequence: command.sequence,
                payload_digest: command.payload_digest.clone(),
                completed_at: Utc::now(),
                outcome: NodeCommandOutcome::Succeeded {
                    result: Box::new(result),
                },
            },
            Utc::now(),
        )
        .await?;
    Ok(())
}

pub(super) fn request(command: &NodeCommandEnvelope) -> TestResult<&NodeBoxBuildRequest> {
    match &command.payload {
        NodeCommandPayload::BoxBuildStart { request }
        | NodeCommandPayload::BoxBuildInspect { request }
        | NodeCommandPayload::BoxBuildCancel { request }
        | NodeCommandPayload::BoxBuildRemove { request } => Ok(request),
        _ => Err("leased command is not a Box build command".into()),
    }
}

pub(super) fn box_output_for(
    request: &NodeBoxBuildRequest,
    output: BuildArtifact,
) -> TestResult<NodeBoxBuildOutput> {
    request.validate()?;
    let artifact = ArtifactRef {
        uri: output.uri,
        digest: output.digest,
        media_type: output.media_type,
    };
    let descriptor = NodeBoxBuildDescriptor {
        media_type: if request.plans.len() == 1 {
            "application/vnd.oci.image.manifest.v1+json".into()
        } else {
            "application/vnd.oci.image.index.v1+json".into()
        },
        digest: runtime::digest('e'),
        size: 512,
    };
    let platforms = request
        .plans
        .iter()
        .map(|plan| {
            let parsed = a3s_box_runtime::BoxBuildPlan::parse_acl(&plan.plan_acl)
                .map_err(|error| error.to_string())?;
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
            let plan_digest = a3s_box_runtime::BoxBuildPlan::parse_acl(&plan.plan_acl)
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
                    key: runtime::digest('3'),
                    source_digest: request.source.digest.clone(),
                    plan_digest,
                    descriptor: descriptor.clone(),
                    platform: platform.clone(),
                    content_bytes: 2048,
                    entry_count: 3,
                    blob_count: 3,
                    blob_inventory_digest: runtime::digest('4'),
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
        blob_inventory_digest: runtime::digest('5'),
        caches,
    };
    output.validate()?;
    Ok(output)
}

fn event(organization_id: OrganizationId, event_key: &str) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: event_key.into(),
        schema_version: 1,
        scope: a3s_cloud_contracts::CloudScopeRef::Organization {
            organization_id: organization_id.as_uuid(),
        },
        aggregate_id: Uuid::now_v7(),
        aggregate_version: 1,
        occurred_at: Utc::now(),
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        payload: serde_json::json!({}),
    }
}
