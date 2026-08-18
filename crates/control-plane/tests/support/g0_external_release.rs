#[path = "g0_external_release_evidence.rs"]
mod evidence;
#[path = "g0_external_release_fixture.rs"]
mod fixture;

use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_control_plane::modules::artifacts::{
    BoxBuildEvidenceGenerator, BuildEvidence, BuildRunFinalization, BuildRunStatus, BuildSource,
    IBuildArtifactPublisher, IBuildEvidenceGenerator, IBuildOutputValidator, IBuildRunRepository,
    NodeArtifactObjectStore, OciPublicationRequest, PostgresBuildRunRepository,
    VaultBuildEvidenceSigner,
};
use a3s_cloud_control_plane::modules::fleet::domain::repositories::INodePoolRepository;
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::projects::domain::repositories::IEnvironmentRepository;
use a3s_cloud_control_plane::modules::projects::PostgresProjectsRepository;
use a3s_cloud_control_plane::modules::secrets::{ISecretRepository, PostgresSecretRepository};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    IdempotencyRequest, NodeCommandId, NodeId,
};
use a3s_cloud_control_plane::modules::sources::domain::{
    AcceptSourceRevision, ISourceRevisionRepository, SourceRevisionAccepted,
};
use a3s_cloud_control_plane::modules::sources::PostgresSourceRevisionRepository;
use a3s_cloud_control_plane::modules::workloads::{
    CreateSourceWorkloadDeployment, CreateSourceWorkloadDeploymentHandler, HttpHealthCheck,
    IWorkloadRepository, PostgresWorkloadRepository, ServicePort, ServiceProcess, ServiceResources,
    SourceWorkloadTemplate,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use fixture::{GateConfig, GateInputs};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(super) const POSTGRES_ENV: &str = "A3S_CLOUD_TEST_POSTGRES_URL";

pub(super) async fn exercise_external_release(database_url: String) -> TestResult {
    let config = GateConfig::load()?;
    let inputs = fixture::load_inputs(&config).await?;
    let executor = fixture::connect(&database_url).await?;
    fixture::seed_tenant(&executor, &inputs.source).await?;
    let sources = Arc::new(PostgresSourceRevisionRepository::new(executor.clone()));
    persist_source(sources.as_ref(), &inputs).await?;
    let builds = Arc::new(PostgresBuildRunRepository::new(executor.clone()));
    let mut build = reserve_build(builds.as_ref(), &inputs).await?;

    let root = tempfile::tempdir()?;
    let artifacts = Arc::new(NodeArtifactObjectStore::local(
        root.path().join("artifacts"),
        256 * 1024 * 1024,
    )?);
    fixture::admit_artifact(
        &artifacts,
        &inputs.source.input_artifact,
        &inputs.source_archive,
    )
    .await?;
    let output_artifact = fixture::admit_box_output(
        &artifacts,
        &inputs.box_release.output,
        &inputs.box_output_archive,
    )
    .await?;
    let outputs = fixture::output_validator(artifacts, root.path())?;
    let validated = outputs
        .validate(&inputs.box_release.output, &inputs.source.revision.recipe)
        .await?;
    if validated.artifact != output_artifact
        || validated.descriptor.digest() != inputs.box_release.output.descriptor.digest
    {
        return Err(test_error(
            "OCI validation changed the exact Box release output",
        ));
    }

    let (node_id, start_command_id, remove_command_id) = fixture::enqueue_box_commands(
        &executor,
        inputs.source.revision.organization_id.as_uuid(),
        &inputs.box_release,
        &config.box_revision,
    )
    .await?;
    let mut at = canonical_time(Utc::now());
    if at < build.updated_at {
        at = build.updated_at;
    }
    build = begin_preparation(builds.as_ref(), build, next_time(&mut at)).await?;
    build = record_input(builds.as_ref(), build, &inputs, next_time(&mut at)).await?;
    build = schedule_build(
        builds.as_ref(),
        build,
        node_id,
        &inputs.box_release.build_request_digest,
        next_time(&mut at),
    )
    .await?;
    build = dispatch_build(builds.as_ref(), build, start_command_id, next_time(&mut at)).await?;
    build = begin_validation(builds.as_ref(), build, &inputs, next_time(&mut at)).await?;
    build = record_validated_output(
        builds.as_ref(),
        build,
        validated.clone(),
        next_time(&mut at),
    )
    .await?;

    let registry = config.registry_settings()?;
    let _registry_credential = config.install_registry_credential()?;
    let publisher = registry.publisher(outputs.clone(), &config.registry_repository_prefix)?;
    let target = publisher.target_for(&build)?;
    build = begin_publication(builds.as_ref(), build, target.clone(), next_time(&mut at)).await?;
    let publication = OciPublicationRequest::new(target.clone(), validated.clone())?;
    let published = publisher.publish(&publication).await?;
    if publisher.find(&publication).await?.as_ref() != Some(&published)
        || publisher.publish(&publication).await? != published
    {
        return Err(test_error(
            "external Registry did not preserve exact digest-addressed replay",
        ));
    }
    let reconstructed_publisher =
        registry.publisher(outputs.clone(), &config.registry_repository_prefix)?;
    if reconstructed_publisher.find(&publication).await?.as_ref() != Some(&published)
        || reconstructed_publisher.publish(&publication).await? != published
    {
        return Err(test_error(
            "reconstructed external Registry publisher did not adopt the exact graph",
        ));
    }
    build = record_publication(
        builds.as_ref(),
        build,
        published.clone(),
        next_time(&mut at),
    )
    .await?;
    build = begin_attestation(builds.as_ref(), build, next_time(&mut at)).await?;

    let signer = Arc::new(VaultBuildEvidenceSigner::new(
        &config.vault_address,
        config.vault_token.as_str(),
        config.vault_transit_mount.clone(),
        config.vault_signing_key.clone(),
        std::time::Duration::from_secs(30),
    )?);
    let evidence_generator = BoxBuildEvidenceGenerator::new(outputs, signer)?;
    let source = BuildSource::from_external_revision(&inputs.source.revision)?;
    let build_evidence = evidence_generator
        .generate(&build, &source, next_time(&mut at))
        .await?;
    let signing_key_version = build_evidence.signing_key.key_version.ok_or_else(|| {
        test_error("Vault Transit build evidence omitted its signing key version")
    })?;
    if BuildEvidence::restore(build_evidence.clone())? != build_evidence {
        return Err(test_error(
            "locally verified Vault DSSE evidence changed during restoration",
        ));
    }
    let evidence_for_certification = build_evidence.clone();
    build = record_evidence(builds.as_ref(), build, build_evidence, next_time(&mut at)).await?;
    build = begin_cleanup(
        builds.as_ref(),
        build,
        remove_command_id,
        next_time(&mut at),
    )
    .await?;
    let expected_version = build.aggregate_version;
    build.complete(next_time(&mut at))?;
    let BuildRunFinalization::Completed(build) = builds.finalize(build, expected_version).await?
    else {
        return Err(test_error(
            "G0 external BuildRun finalization was unexpectedly rejected",
        ));
    };
    if build.status != BuildRunStatus::Succeeded
        || build.published_artifact.as_ref() != Some(&published)
        || build.evidence.as_deref() != Some(&evidence_for_certification)
    {
        return Err(test_error(
            "successful G0 BuildRun omitted its exact publication or evidence",
        ));
    }

    let reconstructed_executor = PostgresExecutor::connect_no_tls(&database_url, 8)?;
    let reconstructed_builds = Arc::new(PostgresBuildRunRepository::new(
        reconstructed_executor.clone(),
    ));
    let restored = reconstructed_builds
        .find(inputs.source.revision.organization_id, build.id)
        .await?;
    if restored != build
        || reconstructed_publisher.find(&publication).await?.as_ref() != Some(&published)
    {
        return Err(test_error(
            "process reconstruction changed the persisted BuildRun or remote OCI graph",
        ));
    }

    let deployment = create_workload_handoff(
        &reconstructed_executor,
        Arc::clone(&reconstructed_builds),
        &inputs,
        next_time(&mut at),
    )
    .await?;
    verify_tracked_release(
        &reconstructed_executor,
        &build,
        &published.digest,
        deployment.bundle.workload.id.as_uuid(),
        deployment.bundle.revision.id.as_uuid(),
        deployment.bundle.deployment.id.as_uuid(),
    )
    .await?;

    evidence::write(
        &config,
        evidence::EvidenceFacts {
            source_repository: inputs.source.revision.repository.canonical_url(),
            source_commit: inputs.source.revision.commit_sha.as_str(),
            source_content_digest: &inputs.source.source_content_digest,
            build_input_digest: &inputs.source.input_artifact.digest,
            box_output_digest: &inputs.box_release.output.descriptor.digest,
            published_artifact_digest: &published.digest,
            published_resource_identity: &published.uri,
            sbom_digest: &evidence_for_certification.sbom_digest,
            provenance_digest: &evidence_for_certification.provenance_digest,
            signing_key_id: &evidence_for_certification.signing_key.key_id,
            signing_key_version,
            build_run_id: build.id.as_uuid(),
            workload_id: deployment.bundle.workload.id.as_uuid(),
            deployment_id: deployment.bundle.deployment.id.as_uuid(),
            source_evidence_digest: &inputs.source_evidence_digest,
            box_evidence_digest: &inputs.box_evidence_digest,
            registry_authority: &registry.authority,
        },
    )
    .await?;
    println!(
        "A3S_CLOUD_G0_EXTERNAL_RELEASE_CERTIFIED build_run={} workload={} deployment={} artifact={} signing_key_version={} store=postgresql",
        build.id,
        deployment.bundle.workload.id,
        deployment.bundle.deployment.id,
        published.digest,
        signing_key_version,
    );
    Ok(())
}

async fn persist_source(
    sources: &PostgresSourceRevisionRepository,
    inputs: &GateInputs,
) -> TestResult {
    let revision = inputs.source.revision.clone();
    let canonical = serde_json::to_vec(&revision)?;
    let idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{}/projects/{}/environments/{}/source-revisions",
            revision.organization_id, revision.project_id, revision.environment_id,
        ),
        "g0-external-release-source",
        &canonical,
    )?;
    let event = SourceRevisionAccepted::envelope(&revision, Uuid::now_v7())?;
    let accepted = sources
        .accept(AcceptSourceRevision {
            revision: revision.clone(),
            webhook_delivery: None,
            idempotency: idempotency.clone(),
            event,
        })
        .await?;
    if accepted.replayed || accepted.value != revision {
        return Err(test_error(
            "PostgreSQL changed the accepted private source revision",
        ));
    }
    let replay = sources
        .replay_acceptance(&idempotency)
        .await?
        .ok_or_else(|| test_error("private source acceptance replay disappeared"))?;
    if replay != revision {
        return Err(test_error(
            "private source acceptance replay changed its immutable identity",
        ));
    }
    Ok(())
}

async fn reserve_build(
    builds: &PostgresBuildRunRepository,
    inputs: &GateInputs,
) -> TestResult<a3s_cloud_control_plane::modules::artifacts::BuildRun> {
    let reserved = builds
        .reserve_pending(1, canonical_time(Utc::now()))
        .await?;
    if reserved.len() != 1 || reserved[0].id != inputs.source.build_run_id {
        return Err(test_error(
            "PostgreSQL did not reserve the exact private-source BuildRun",
        ));
    }
    Ok(reserved.into_iter().next().expect("checked one BuildRun"))
}

async fn begin_preparation(
    builds: &PostgresBuildRunRepository,
    mut build: a3s_cloud_control_plane::modules::artifacts::BuildRun,
    at: DateTime<Utc>,
) -> TestResult<a3s_cloud_control_plane::modules::artifacts::BuildRun> {
    let expected = build.aggregate_version;
    build.begin_preparation(at)?;
    Ok(builds.save(build, expected).await?)
}

async fn record_input(
    builds: &PostgresBuildRunRepository,
    mut build: a3s_cloud_control_plane::modules::artifacts::BuildRun,
    inputs: &GateInputs,
    at: DateTime<Utc>,
) -> TestResult<a3s_cloud_control_plane::modules::artifacts::BuildRun> {
    let expected = build.aggregate_version;
    build.record_input(
        inputs.source.source_content_digest.clone(),
        inputs.source.input_artifact.clone(),
        at,
    )?;
    Ok(builds.save(build, expected).await?)
}

async fn schedule_build(
    builds: &PostgresBuildRunRepository,
    mut build: a3s_cloud_control_plane::modules::artifacts::BuildRun,
    node_id: NodeId,
    request_digest: &str,
    at: DateTime<Utc>,
) -> TestResult<a3s_cloud_control_plane::modules::artifacts::BuildRun> {
    let expected = build.aggregate_version;
    build.schedule(node_id, request_digest.to_owned(), at)?;
    Ok(builds.save(build, expected).await?)
}

async fn dispatch_build(
    builds: &PostgresBuildRunRepository,
    mut build: a3s_cloud_control_plane::modules::artifacts::BuildRun,
    command_id: NodeCommandId,
    at: DateTime<Utc>,
) -> TestResult<a3s_cloud_control_plane::modules::artifacts::BuildRun> {
    let expected = build.aggregate_version;
    build.dispatch(command_id, at)?;
    Ok(builds.save(build, expected).await?)
}

async fn begin_validation(
    builds: &PostgresBuildRunRepository,
    mut build: a3s_cloud_control_plane::modules::artifacts::BuildRun,
    inputs: &GateInputs,
    at: DateTime<Utc>,
) -> TestResult<a3s_cloud_control_plane::modules::artifacts::BuildRun> {
    let expected = build.aggregate_version;
    build.begin_validation(inputs.box_release.output.clone(), at)?;
    Ok(builds.save(build, expected).await?)
}

async fn record_validated_output(
    builds: &PostgresBuildRunRepository,
    mut build: a3s_cloud_control_plane::modules::artifacts::BuildRun,
    output: a3s_cloud_control_plane::modules::artifacts::ValidatedOciBuildOutput,
    at: DateTime<Utc>,
) -> TestResult<a3s_cloud_control_plane::modules::artifacts::BuildRun> {
    let expected = build.aggregate_version;
    build.record_validated_output(output, at)?;
    Ok(builds.save(build, expected).await?)
}

async fn begin_publication(
    builds: &PostgresBuildRunRepository,
    mut build: a3s_cloud_control_plane::modules::artifacts::BuildRun,
    target: a3s_cloud_control_plane::modules::artifacts::OciPublicationTarget,
    at: DateTime<Utc>,
) -> TestResult<a3s_cloud_control_plane::modules::artifacts::BuildRun> {
    let expected = build.aggregate_version;
    build.begin_publication(target, at)?;
    Ok(builds.save(build, expected).await?)
}

async fn record_publication(
    builds: &PostgresBuildRunRepository,
    mut build: a3s_cloud_control_plane::modules::artifacts::BuildRun,
    artifact: a3s_cloud_control_plane::modules::artifacts::PublishedOciArtifact,
    at: DateTime<Utc>,
) -> TestResult<a3s_cloud_control_plane::modules::artifacts::BuildRun> {
    let expected = build.aggregate_version;
    build.record_published_artifact(artifact, at)?;
    Ok(builds.save(build, expected).await?)
}

async fn begin_attestation(
    builds: &PostgresBuildRunRepository,
    mut build: a3s_cloud_control_plane::modules::artifacts::BuildRun,
    at: DateTime<Utc>,
) -> TestResult<a3s_cloud_control_plane::modules::artifacts::BuildRun> {
    let expected = build.aggregate_version;
    build.begin_attestation(at)?;
    Ok(builds.save(build, expected).await?)
}

async fn record_evidence(
    builds: &PostgresBuildRunRepository,
    mut build: a3s_cloud_control_plane::modules::artifacts::BuildRun,
    evidence: BuildEvidence,
    at: DateTime<Utc>,
) -> TestResult<a3s_cloud_control_plane::modules::artifacts::BuildRun> {
    let expected = build.aggregate_version;
    build.record_evidence(evidence, at)?;
    Ok(builds.save(build, expected).await?)
}

async fn begin_cleanup(
    builds: &PostgresBuildRunRepository,
    mut build: a3s_cloud_control_plane::modules::artifacts::BuildRun,
    command_id: NodeCommandId,
    at: DateTime<Utc>,
) -> TestResult<a3s_cloud_control_plane::modules::artifacts::BuildRun> {
    let expected = build.aggregate_version;
    build.begin_cleanup(command_id, at)?;
    Ok(builds.save(build, expected).await?)
}

async fn create_workload_handoff(
    executor: &PostgresExecutor,
    builds: Arc<PostgresBuildRunRepository>,
    inputs: &GateInputs,
    requested_at: DateTime<Utc>,
) -> TestResult<a3s_cloud_control_plane::modules::workloads::CreateSourceWorkloadDeploymentResult> {
    let environments: Arc<dyn IEnvironmentRepository> =
        Arc::new(PostgresProjectsRepository::new(executor.clone()));
    let sources: Arc<dyn ISourceRevisionRepository> =
        Arc::new(PostgresSourceRevisionRepository::new(executor.clone()));
    let builds: Arc<dyn IBuildRunRepository> = builds;
    let workloads: Arc<dyn IWorkloadRepository> =
        Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let secrets: Arc<dyn ISecretRepository> =
        Arc::new(PostgresSecretRepository::new(executor.clone()));
    let node_pools: Arc<dyn INodePoolRepository> =
        Arc::new(PostgresNodeRepository::new(executor.clone()));
    let handler = CreateSourceWorkloadDeploymentHandler::new(
        environments,
        sources,
        builds,
        workloads,
        secrets,
        node_pools,
    );
    let template = source_workload_template();
    let first = execute_workload(
        &handler,
        source_workload_command(inputs, template.clone(), requested_at),
    )
    .await?;
    let replay = execute_workload(
        &handler,
        source_workload_command(
            inputs,
            template,
            requested_at + ChronoDuration::milliseconds(1),
        ),
    )
    .await?;
    if first.bundle.replayed
        || !replay.bundle.replayed
        || first.bundle.workload.id != replay.bundle.workload.id
        || first.bundle.revision.id != replay.bundle.revision.id
        || first.bundle.deployment.id != replay.bundle.deployment.id
        || first.bundle.operation.id != replay.bundle.operation.id
    {
        return Err(test_error(
            "published Workload handoff did not preserve exact idempotency replay",
        ));
    }
    let external = first
        .bundle
        .revision
        .external_build
        .as_ref()
        .ok_or_else(|| test_error("published Workload omitted its external build trace"))?;
    if external.source_revision_id != inputs.source.revision.id
        || external.build_run_id != inputs.source.build_run_id
        || first.bundle.operation.workflow.name() != "cloud.deployment"
        || first.bundle.operation.workflow.version() != "4"
    {
        return Err(test_error(
            "published Workload changed its BuildRun or deployment workflow identity",
        ));
    }
    Ok(first)
}

fn source_workload_command(
    inputs: &GateInputs,
    template: SourceWorkloadTemplate,
    requested_at: DateTime<Utc>,
) -> CreateSourceWorkloadDeployment {
    CreateSourceWorkloadDeployment {
        organization_id: inputs.source.revision.organization_id,
        project_id: inputs.source.revision.project_id,
        environment_id: inputs.source.revision.environment_id,
        source_revision_id: inputs.source.revision.id,
        name: "G0 published workload".into(),
        node_pool_id: None,
        template,
        idempotency_key: "g0-external-release-workload".into(),
        request_id: Uuid::now_v7(),
        requested_at,
    }
}

fn source_workload_template() -> SourceWorkloadTemplate {
    SourceWorkloadTemplate {
        process: ServiceProcess {
            command: Vec::new(),
            args: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
        },
        secrets: Vec::new(),
        resources: ServiceResources {
            cpu_millis: 100,
            memory_bytes: 32 * 1024 * 1024,
            pids: 32,
            ephemeral_storage_bytes: None,
        },
        ports: Vec::<ServicePort>::new(),
        health: None::<HttpHealthCheck>,
    }
}

async fn execute_workload(
    handler: &CreateSourceWorkloadDeploymentHandler,
    command: CreateSourceWorkloadDeployment,
) -> TestResult<a3s_cloud_control_plane::modules::workloads::CreateSourceWorkloadDeploymentResult> {
    handler
        .execute(command, CqrsContext::new(ModuleRef::new()))
        .await?
        .map_err(|error| test_error(error.to_string()))
}

async fn verify_tracked_release(
    executor: &PostgresExecutor,
    build: &a3s_cloud_control_plane::modules::artifacts::BuildRun,
    artifact_digest: &str,
    workload_id: Uuid,
    revision_id: Uuid,
    deployment_id: Uuid,
) -> TestResult {
    let database = Database::new(PostgresDialect, executor.clone());
    let tracked = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from build_runs b join workload_revisions r on r.external_build_run_id = b.id join deployments d on d.revision_id = r.id join operation_requests o on o.operation_id = d.operation_id where b.id = ",
            )
            .bind(build.id.as_uuid())
            .append(" and b.status = 'succeeded' and b.evidence is not null and b.published_artifact ->> 'digest' = ")
            .bind(artifact_digest)
            .append(" and r.workload_id = ")
            .bind(workload_id)
            .append(" and r.id = ")
            .bind(revision_id)
            .append(" and d.id = ")
            .bind(deployment_id)
            .append(" and o.workflow_name = 'cloud.deployment' and o.workflow_version = '3' and o.input ->> 'buildRunId' = ")
            .bind(build.id.to_string())
            .append(" and o.input ->> 'publishedArtifactDigest' = ")
            .bind(artifact_digest),
        )
        .await?;
    let deployment_events = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from outbox_events where aggregate_id = ")
                .bind(deployment_id)
                .append(" and event_key = 'workload.deployment.requested'"),
        )
        .await?;
    if tracked != 1 || deployment_events != 1 {
        return Err(test_error(
            "published external OCI resource is not tracked by one durable Workload handoff",
        ));
    }
    Ok(())
}

fn next_time(at: &mut DateTime<Utc>) -> DateTime<Utc> {
    *at = canonical_time(*at + ChronoDuration::milliseconds(1));
    *at
}

fn canonical_time(value: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(value.timestamp_micros())
        .expect("PostgreSQL-compatible G0 timestamp")
}

fn test_error(message: impl Into<String>) -> Box<dyn std::error::Error> {
    std::io::Error::other(message.into()).into()
}
