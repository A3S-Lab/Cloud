use super::build_plan::project_build_request;
use super::{BuildFlowConfig, BuildFlowConfigOptions};
use crate::modules::artifacts::domain::{BuildArtifact, BuildRun};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, SourceRevisionId,
};
use crate::modules::sources::domain::{
    BuildRecipe, ExternalSourceRevision, GitCommitSha, GitProvider, GitRepository,
    NewExternalSourceRevision,
};
use a3s_cloud_contracts::NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE;
use chrono::{Duration, Utc};

mod flow;
mod support;

#[test]
fn projection_emits_one_canonical_box_plan_and_no_runtime_task(
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut build, revision) = prepared_build()?;
    let config = config()?;

    let request = project_build_request(&config, &build, &revision, None)?;
    request.validate()?;
    assert_eq!(request.schema, "a3s.cloud.box-build-request.v1");
    assert_eq!(request.plans.len(), 1);
    assert_eq!(
        request.source.digest,
        build
            .input_artifact
            .as_ref()
            .expect("prepared BuildRun must retain its input Artifact")
            .digest
    );
    assert_eq!(request.output_max_bytes, config.output_max_bytes);
    assert_eq!(request.cache_max_bytes, config.cache_max_bytes);
    assert!(request.plans[0].cache.is_none());
    assert_eq!(
        request.plans[0].plan_acl,
        concat!(
            "build \"oci\" {\n",
            "  cache = \"content-addressed\"\n",
            "  context = \".\"\n",
            "  file = \"Dockerfile\"\n",
            "  network = \"none\"\n",
            "  platform = \"linux/amd64\"\n",
            "  schema = \"a3s.box.build-plan.v1\"\n",
            "}\n",
        )
    );

    let request_digest = request.binding_digest()?;
    build.schedule(NodeId::new(), request_digest.clone(), Utc::now())?;
    assert_eq!(
        build.build_request_digest.as_deref(),
        Some(request_digest.as_str())
    );
    assert!(serde_json::to_string(&request)?.contains("planAcl"));
    assert!(!serde_json::to_string(&request)?.contains("RuntimeApply"));
    Ok(())
}

#[test]
fn retry_reuses_only_the_parent_box_cache_receipt() -> Result<(), Box<dyn std::error::Error>> {
    let (mut parent, revision) = prepared_build()?;
    let config = config()?;
    let parent_request = project_build_request(&config, &parent, &revision, None)?;
    parent.schedule(NodeId::new(), parent_request.binding_digest()?, Utc::now())?;
    parent.dispatch(NodeCommandId::new(), Utc::now())?;
    let parent_output = support::box_output_for(&parent_request, support::artifact('2', 8192)?)?;
    parent.begin_validation(parent_output.clone(), Utc::now())?;
    parent.record_failure("fixture failure after Box output".into(), Utc::now())?;
    parent.begin_cleanup(NodeCommandId::new(), Utc::now())?;
    parent.complete(Utc::now())?;

    let mut retry = BuildRun::retry(&parent, Utc::now() + Duration::milliseconds(1))?;
    retry.begin_preparation(Utc::now() + Duration::milliseconds(2))?;
    retry.record_input(
        parent
            .source_content_digest
            .clone()
            .ok_or("source digest")?,
        parent.input_artifact.clone().ok_or("input artifact")?,
        Utc::now() + Duration::milliseconds(3),
    )?;

    let request = project_build_request(&config, &retry, &revision, Some(&parent_output))?;
    let cache = request.plans[0]
        .cache
        .as_ref()
        .ok_or("retry omitted its parent Box cache")?;
    assert_eq!(cache.receipt, parent_output.caches[0].receipt);
    assert_eq!(cache.artifact, parent_output.caches[0].artifact.artifact);
    assert_eq!(cache.receipt.source_digest, request.source.digest);

    let mut changed = parent_output;
    changed.caches[0].receipt.plan_digest = support::digest('9');
    assert!(project_build_request(&config, &retry, &revision, Some(&changed)).is_err());
    Ok(())
}

fn prepared_build() -> Result<(BuildRun, ExternalSourceRevision), Box<dyn std::error::Error>> {
    let revision_id = SourceRevisionId::new();
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let now = Utc::now();
    let revision = ExternalSourceRevision::accept(NewExternalSourceRevision {
        organization_id,
        project_id,
        environment_id,
        id: revision_id,
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
        accepted_at: now,
    })?;
    let mut build = BuildRun::reserve(
        organization_id,
        project_id,
        environment_id,
        revision_id,
        now,
    );
    build.begin_preparation(now)?;
    build.record_input(
        support::digest('1'),
        BuildArtifact::new(
            format!("a3s-cloud-artifact://sha256/{}", "1".repeat(64)),
            support::digest('1'),
            NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
            4096,
        )?,
        now,
    )?;
    Ok((build, revision))
}

fn config() -> Result<BuildFlowConfig, String> {
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
