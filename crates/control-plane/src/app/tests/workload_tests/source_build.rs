use super::*;
use crate::modules::artifacts::domain::test_support::evidence_for;
use crate::modules::artifacts::{
    BuildArtifact, BuildRun, InMemoryBuildRunRepository, OciDescriptor, OciPublicationTarget,
    PublishedOciArtifact, ValidatedBuildCache, ValidatedOciBuildOutput,
};
use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId, SourceRevisionId};
use crate::modules::sources::domain::BuildPlatform;

#[tokio::test]
async fn source_build_deployment_requires_one_owned_success_and_replays_exactly() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let secrets = Arc::new(InMemorySecretRepository::new());
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let sources = Arc::new(InMemorySourceRevisionRepository::new());
    let builds = Arc::new(InMemoryBuildRunRepository::new());
    let app = build_test_application_with_external_builds(
        identity,
        Arc::clone(&projects),
        secrets,
        Arc::clone(&workloads),
        Arc::clone(&sources),
        Arc::clone(&builds),
    )?;
    let organization = bootstrap_organization(&app, "source-deploy-bootstrap", "Acme").await?;
    let project = create_project(&app, &organization, "source-deploy-project", "Cloud").await?;
    let environment_path =
        format!("/api/v1/organizations/{organization}/projects/{project}/environments");
    let environment = app
        .call(post_json(
            &environment_path,
            "source-deploy-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment = response_id(&environment)?;
    let wrong_environment = app
        .call(post_json(
            &environment_path,
            "source-deploy-wrong-environment",
            json!({"name": "Staging"}),
        ))
        .await?;
    assert_eq!(wrong_environment.status(), 201);
    let wrong_environment = response_id(&wrong_environment)?;

    let organization_id = parse_organization_id(&organization)?;
    let project_id = parse_project_id(&project)?;
    let environment_id = parse_environment_id(&environment)?;
    let source_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/source-revisions"
    );
    let queued_source = accept_source(&app, &source_path, "source-deploy-queued", 'a').await?;
    let queued_source_id = parse_source_revision_id(&queued_source)?;
    let workload_path = source_workload_path(&organization, &project, &environment, &queued_source);
    let body = source_workload_body("api");
    assert!(body["template"].get("artifact").is_none());

    let mut injected_artifact = body.clone();
    injected_artifact["template"]["artifact"] = json!({
        "uri": format!("oci://registry.example.test/forged@{}", digest('f')),
        "expectedDigest": digest('f')
    });
    let rejected_artifact = app
        .call(post_json(
            &workload_path,
            "source-deploy-forged-artifact",
            injected_artifact,
        ))
        .await?;
    assert_eq!(rejected_artifact.status(), 400);

    let unreserved = app
        .call(post_json(
            &workload_path,
            "source-deploy-workload",
            body.clone(),
        ))
        .await?;
    assert_eq!(unreserved.status(), 409);
    assert!(workloads
        .list_workloads(organization_id, project_id, environment_id)
        .await
        .map_err(repository_error)?
        .is_empty());

    let queued = reserve_build(
        builds.as_ref(),
        organization_id,
        project_id,
        environment_id,
        queued_source_id,
    )
    .await?;
    let pending = app
        .call(post_json(
            &workload_path,
            "source-deploy-workload",
            body.clone(),
        ))
        .await?;
    assert_eq!(pending.status(), 409);
    assert!(workloads
        .list_workloads(organization_id, project_id, environment_id)
        .await
        .map_err(repository_error)?
        .is_empty());

    let succeeded = succeed_build(builds.as_ref(), queued).await?;
    let published = succeeded
        .published_artifact
        .as_ref()
        .ok_or_else(|| BootError::Internal("succeeded test build has no publication".into()))?;
    let expected_evidence = succeeded
        .evidence
        .as_deref()
        .ok_or_else(|| BootError::Internal("succeeded test build has no evidence".into()))?;
    let build_path = format!(
        "/api/v1/organizations/{organization}/build-runs/{}",
        succeeded.id
    );
    let build_detail = app.call(get_as(&build_path, ADMIN_TOKEN)).await?;
    assert_eq!(build_detail.status(), 200);
    let build_detail = response_json(&build_detail)?;
    assert_eq!(
        build_detail["data"]["evidenceSummary"]["verificationState"],
        "verified"
    );
    assert_eq!(
        build_detail["data"]["evidenceSummary"]["sbomDigest"],
        expected_evidence.sbom_digest
    );
    assert_eq!(
        build_detail["data"]["evidenceSummary"]["provenanceDigest"],
        expected_evidence.provenance_digest
    );
    assert_eq!(
        build_detail["data"]["evidenceSummary"]["signingKeyId"],
        expected_evidence.signing_key.key_id
    );
    let evidence = app
        .call(get_as(format!("{build_path}/evidence"), ADMIN_TOKEN))
        .await?;
    assert_eq!(evidence.status(), 200);
    assert_eq!(
        response_json(&evidence)?["data"],
        serde_json::to_value(expected_evidence)
            .map_err(|error| BootError::Internal(error.to_string()))?
    );
    let accepted = app
        .call(post_json(
            &workload_path,
            "source-deploy-workload",
            body.clone(),
        ))
        .await?;
    let replayed = app
        .call(post_json(
            &workload_path,
            "source-deploy-workload",
            body.clone(),
        ))
        .await?;
    assert_eq!(accepted.status(), 202);
    assert_eq!(replayed.status(), 200);
    let accepted_json = response_json(&accepted)?;
    let replayed_json = response_json(&replayed)?;
    assert_eq!(
        accepted_json["data"]["externalSourceRevisionId"],
        queued_source
    );
    assert_eq!(
        accepted_json["data"]["buildRunId"],
        succeeded.id.to_string()
    );
    assert_eq!(accepted_json["data"]["artifactSourceUri"], published.uri);
    assert_eq!(
        accepted_json["data"]["expectedArtifactDigest"],
        published.digest
    );
    assert_eq!(accepted_json["data"]["artifactDigest"], published.digest);
    assert_eq!(
        accepted_json["data"]["deploymentId"],
        replayed_json["data"]["deploymentId"]
    );
    assert_eq!(replayed_json["data"]["replayed"], true);

    let changed_replay = app
        .call(post_json(
            &workload_path,
            "source-deploy-workload",
            source_workload_body("changed"),
        ))
        .await?;
    assert_eq!(changed_replay.status(), 409);
    let workload_id = accepted_json["data"]["workloadId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("source deployment omitted workload ID".into()))?;
    let detail = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/workloads/{workload_id}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(detail.status(), 200);
    let detail = response_json(&detail)?;
    assert_eq!(
        detail["data"]["desiredRevision"]["externalSourceRevisionId"],
        queued_source
    );
    assert_eq!(
        detail["data"]["desiredRevision"]["buildRunId"],
        succeeded.id.to_string()
    );

    let cross_environment = app
        .call(post_json(
            source_workload_path(&organization, &project, &wrong_environment, &queued_source),
            "source-deploy-cross-environment",
            source_workload_body("cross-environment"),
        ))
        .await?;
    assert_eq!(cross_environment.status(), 404);

    for (suffix, commit, terminal) in [
        ("failed", 'b', TerminalBuild::Failed),
        ("cancelled", 'c', TerminalBuild::Cancelled),
    ] {
        let source = accept_source(
            &app,
            &source_path,
            &format!("source-deploy-{suffix}"),
            commit,
        )
        .await?;
        let source_id = parse_source_revision_id(&source)?;
        let build = reserve_build(
            builds.as_ref(),
            organization_id,
            project_id,
            environment_id,
            source_id,
        )
        .await?;
        finish_unsuccessfully(builds.as_ref(), build, terminal).await?;
        let rejected = app
            .call(post_json(
                source_workload_path(&organization, &project, &environment, &source),
                &format!("source-deploy-{suffix}-workload"),
                source_workload_body(suffix),
            ))
            .await?;
        assert_eq!(rejected.status(), 409);
    }
    assert_eq!(
        workloads
            .list_workloads(organization_id, project_id, environment_id)
            .await
            .map_err(repository_error)?
            .len(),
        1
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum TerminalBuild {
    Failed,
    Cancelled,
}

async fn accept_source(
    app: &a3s_boot::BootApplication,
    path: &str,
    idempotency_key: &str,
    commit_character: char,
) -> Result<String> {
    let response = app
        .call(post_json(
            path,
            idempotency_key,
            json!({
                "repository": {
                    "provider": "github",
                    "url": "https://github.com/A3S-Lab/Cloud"
                },
                "reference": {
                    "kind": "commit",
                    "value": commit_character.to_string().repeat(40)
                },
                "recipe": {
                    "schema": "a3s.cloud.build-recipe.v1",
                    "kind": "dockerfile",
                    "contextPath": ".",
                    "dockerfilePath": "Dockerfile",
                    "target": null,
                    "platforms": ["linux/amd64"]
                },
                "webhookDeliveryId": format!("source-deploy-{commit_character}")
            }),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    response_id(&response)
}

async fn reserve_build(
    builds: &InMemoryBuildRunRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    source_revision_id: SourceRevisionId,
) -> Result<BuildRun> {
    let accepted_at = Utc::now();
    builds
        .add_source_revision(
            organization_id,
            project_id,
            environment_id,
            source_revision_id,
            accepted_at,
        )
        .await;
    builds
        .reserve_pending(1, accepted_at)
        .await
        .map_err(repository_error)?
        .pop()
        .ok_or_else(|| BootError::Internal("test build was not reserved".into()))
}

async fn succeed_build(
    builds: &InMemoryBuildRunRepository,
    mut build: BuildRun,
) -> Result<BuildRun> {
    let mut at = build.updated_at;
    let previous = build.aggregate_version;
    at += chrono::Duration::milliseconds(1);
    build.begin_preparation(at).map_err(BootError::Internal)?;
    let mut build = builds
        .save(build, previous)
        .await
        .map_err(repository_error)?;
    let previous = build.aggregate_version;
    at += chrono::Duration::milliseconds(1);
    build
        .record_input(digest('a'), build_artifact('b')?, at)
        .map_err(BootError::Internal)?;
    let mut build = builds
        .save(build, previous)
        .await
        .map_err(repository_error)?;
    let previous = build.aggregate_version;
    at += chrono::Duration::milliseconds(1);
    build
        .schedule(NodeId::new(), digest('c'), at)
        .map_err(BootError::Internal)?;
    let mut build = builds
        .save(build, previous)
        .await
        .map_err(repository_error)?;
    let previous = build.aggregate_version;
    at += chrono::Duration::milliseconds(1);
    build
        .dispatch(NodeCommandId::new(), at)
        .map_err(BootError::Internal)?;
    let mut build = builds
        .save(build, previous)
        .await
        .map_err(repository_error)?;
    let previous = build.aggregate_version;
    at += chrono::Duration::milliseconds(1);
    let output_artifact = build_artifact('d')?;
    build
        .begin_validation(output_artifact.clone(), at)
        .map_err(BootError::Internal)?;
    let mut build = builds
        .save(build, previous)
        .await
        .map_err(repository_error)?;
    let descriptor = OciDescriptor::new(
        "application/vnd.oci.image.manifest.v1+json",
        digest('e'),
        512,
    )
    .map_err(BootError::Internal)?;
    let output = ValidatedOciBuildOutput {
        artifact: output_artifact,
        descriptor: descriptor.clone(),
        platforms: vec![BuildPlatform::parse("linux/amd64").map_err(BootError::Internal)?],
        content_bytes: 1_024,
        blob_count: 3,
    };
    let cache = ValidatedBuildCache::new(
        digest('f'),
        output.artifact.clone(),
        OciDescriptor::new("application/vnd.oci.image.index.v1+json", digest('9'), 256)
            .map_err(BootError::Internal)?,
        512,
        2,
    )
    .map_err(BootError::Internal)?;
    let previous = build.aggregate_version;
    at += chrono::Duration::milliseconds(1);
    build
        .record_validated_output(output, Some(cache), at)
        .map_err(BootError::Internal)?;
    let mut build = builds
        .save(build, previous)
        .await
        .map_err(repository_error)?;
    let target = OciPublicationTarget::new(
        "registry.example.test",
        format!("a3s/builds/{}", build.id),
        descriptor,
    )
    .map_err(BootError::Internal)?;
    let previous = build.aggregate_version;
    at += chrono::Duration::milliseconds(1);
    build
        .begin_publication(target.clone(), at)
        .map_err(BootError::Internal)?;
    let mut build = builds
        .save(build, previous)
        .await
        .map_err(repository_error)?;
    let previous = build.aggregate_version;
    at += chrono::Duration::milliseconds(1);
    build
        .record_published_artifact(PublishedOciArtifact::from_target(&target), at)
        .map_err(BootError::Internal)?;
    let mut build = builds
        .save(build, previous)
        .await
        .map_err(repository_error)?;
    let previous = build.aggregate_version;
    at += chrono::Duration::milliseconds(1);
    build.begin_attestation(at).map_err(BootError::Internal)?;
    let mut build = builds
        .save(build, previous)
        .await
        .map_err(repository_error)?;
    let previous = build.aggregate_version;
    at += chrono::Duration::milliseconds(1);
    let evidence = evidence_for(&build, at);
    build
        .record_evidence(evidence, at)
        .map_err(BootError::Internal)?;
    let mut build = builds
        .save(build, previous)
        .await
        .map_err(repository_error)?;
    let previous = build.aggregate_version;
    at += chrono::Duration::milliseconds(1);
    build
        .begin_cleanup(NodeCommandId::new(), at)
        .map_err(BootError::Internal)?;
    let mut build = builds
        .save(build, previous)
        .await
        .map_err(repository_error)?;
    let previous = build.aggregate_version;
    at += chrono::Duration::milliseconds(1);
    build.complete(at).map_err(BootError::Internal)?;
    builds.save(build, previous).await.map_err(repository_error)
}

async fn finish_unsuccessfully(
    builds: &InMemoryBuildRunRepository,
    mut build: BuildRun,
    terminal: TerminalBuild,
) -> Result<BuildRun> {
    let previous = build.aggregate_version;
    let at = build.updated_at + chrono::Duration::milliseconds(1);
    match terminal {
        TerminalBuild::Failed => build
            .record_failure("test build failed".into(), at)
            .map_err(BootError::Internal)?,
        TerminalBuild::Cancelled => build
            .request_cancellation(at)
            .map_err(BootError::Internal)?,
    }
    let mut build = builds
        .save(build, previous)
        .await
        .map_err(repository_error)?;
    let previous = build.aggregate_version;
    build
        .complete(at + chrono::Duration::milliseconds(1))
        .map_err(BootError::Internal)?;
    builds.save(build, previous).await.map_err(repository_error)
}

fn build_artifact(character: char) -> Result<BuildArtifact> {
    BuildArtifact::new(
        format!("memory://build-artifact/{character}"),
        digest(character),
        "application/vnd.a3s.directory.v1",
        1_024,
    )
    .map_err(BootError::Internal)
}

fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}

fn source_workload_path(
    organization: &str,
    project: &str,
    environment: &str,
    source_revision: &str,
) -> String {
    format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/source-revisions/{source_revision}/workloads"
    )
}

fn source_workload_body(name: &str) -> Value {
    json!({
        "name": name,
        "template": {
            "process": {
                "command": [],
                "args": [],
                "workingDirectory": null,
                "environment": {}
            },
            "secrets": [],
            "resources": {
                "cpuMillis": 100,
                "memoryBytes": 33554432,
                "pids": 32,
                "ephemeralStorageBytes": null
            },
            "ports": [{"name": "http", "containerPort": 8080}],
            "health": {
                "portName": "http",
                "path": "/health",
                "intervalMs": 1000,
                "timeoutMs": 500,
                "healthyThreshold": 1,
                "unhealthyThreshold": 3,
                "stabilizationWindowMs": 1000
            }
        }
    })
}

fn parse_project_id(value: &str) -> Result<ProjectId> {
    Uuid::parse_str(value)
        .map(ProjectId::from_uuid)
        .map_err(|error| BootError::Internal(error.to_string()))
}

fn parse_environment_id(value: &str) -> Result<EnvironmentId> {
    Uuid::parse_str(value)
        .map(EnvironmentId::from_uuid)
        .map_err(|error| BootError::Internal(error.to_string()))
}

fn parse_source_revision_id(value: &str) -> Result<SourceRevisionId> {
    Uuid::parse_str(value)
        .map(SourceRevisionId::from_uuid)
        .map_err(|error| BootError::Internal(error.to_string()))
}
