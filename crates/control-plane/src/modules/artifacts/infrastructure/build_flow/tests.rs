use super::task_spec::{
    build_cache_key, project_task_spec, BUILDKIT_ADDRESS, CACHE_IMPORT_ROOT, CACHE_ROOT,
    OUTPUT_NAME,
};
use super::BuildFlowConfig;
use crate::modules::artifacts::domain::{
    BuildArtifact, BuildRun, OciDescriptor, ValidatedBuildCache, ValidatedOciBuildOutput,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, SourceRevisionId,
};
use crate::modules::sources::domain::{
    BuildPlatform, BuildRecipe, ExternalSourceRevision, GitCommitSha, GitProvider, GitRepository,
    NewExternalSourceRevision,
};
use a3s_cloud_contracts::NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE;
use a3s_runtime::contract::{ArtifactRef, NetworkMode, RuntimeMountSource, RuntimeUnitClass};
use chrono::{Duration, Utc};

mod attestation;
mod flow;
mod runtime_gate;
mod support;

#[test]
fn projected_build_task_has_two_independent_network_denials_and_exact_artifacts(
) -> Result<(), Box<dyn std::error::Error>> {
    let revision_id = SourceRevisionId::new();
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
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
        accepted_at: Utc::now(),
    })?;
    let mut build = BuildRun::reserve(
        organization_id,
        project_id,
        environment_id,
        revision_id,
        Utc::now(),
    );
    build.begin_preparation(Utc::now())?;
    let input_digest = format!("sha256:{}", "b".repeat(64));
    build.record_input(
        input_digest.clone(),
        BuildArtifact::new(
            format!("a3s-cloud-artifact://sha256/{}", "c".repeat(64)),
            format!("sha256:{}", "c".repeat(64)),
            NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
            4096,
        )?,
        Utc::now(),
    )?;
    let builder_digest = format!("sha256:{}", "d".repeat(64));
    let config = BuildFlowConfig::new(super::BuildFlowConfigOptions {
        builder: ArtifactRef {
            uri: format!("oci://docker.io/moby/buildkit@{builder_digest}"),
            digest: builder_digest,
            media_type: "application/vnd.oci.image.index.v1+json".into(),
        },
        buildkit_socket_volume_id: "a3s-cloud-buildkit-v0-31-2".into(),
        heartbeat_timeout_ms: 10_000,
        command_ttl_ms: 120_000,
        execution_timeout_ms: 60_000,
        observation_poll_ms: 100,
        convergence_timeout_ms: 120_000,
        cleanup_timeout_ms: 60_000,
        publication_timeout_ms: 60_000,
        cpu_millis: 1_000,
        memory_bytes: 512 * 1024 * 1024,
        pids: 256,
        output_max_bytes: 128 * 1024 * 1024,
    })?;

    let spec = project_task_spec(&config, &build, &revision, None)?;
    assert_eq!(spec.class, RuntimeUnitClass::Task);
    assert_eq!(spec.network.mode, NetworkMode::None);
    assert!(spec
        .process
        .args
        .windows(2)
        .any(|arguments| { arguments == ["--opt", "force-network-mode=none"] }));
    assert!(spec
        .process
        .args
        .windows(2)
        .any(|arguments| { arguments == ["--addr", BUILDKIT_ADDRESS] }));
    assert!(!spec
        .process
        .args
        .iter()
        .any(|argument| matches!(argument.as_str(), "--allow" | "--ssh" | "--secret")));
    assert!(!spec
        .process
        .args
        .iter()
        .any(|argument| argument == "--import-cache"));
    assert!(spec
        .process
        .args
        .iter()
        .any(|argument| argument == "--export-cache"));
    assert_eq!(
        spec.process.environment.get("A3S_BUILD_CACHE_KEY"),
        Some(&build_cache_key(&config, &build, &revision)?)
    );
    assert!(!spec
        .process
        .environment
        .contains_key("A3S_BUILD_CACHE_IMPORT"));
    assert_eq!(spec.mounts.len(), 2);
    assert!(matches!(
        &spec.mounts[0].source,
        RuntimeMountSource::Artifact { artifact }
            if artifact.digest == build.input_artifact.as_ref().expect("input").digest
                && spec.mounts[0].read_only
    ));
    assert!(matches!(
        &spec.mounts[1].source,
        RuntimeMountSource::Volume { volume_id }
            if volume_id == "a3s-cloud-buildkit-v0-31-2" && spec.mounts[1].read_only
    ));
    assert_eq!(spec.outputs.len(), 1);
    assert_eq!(spec.outputs[0].name, OUTPUT_NAME);
    assert_eq!(
        spec.outputs[0].media_type,
        NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE
    );

    let mut parent = build;
    parent.schedule(NodeId::new(), spec.digest()?, Utc::now())?;
    parent.dispatch(NodeCommandId::new(), Utc::now())?;
    let runtime_output = BuildArtifact::new(
        format!("a3s-cloud-artifact://sha256/{}", "e".repeat(64)),
        format!("sha256:{}", "e".repeat(64)),
        NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
        8192,
    )?;
    parent.begin_validation(runtime_output.clone(), Utc::now())?;
    let output = ValidatedOciBuildOutput {
        artifact: runtime_output.clone(),
        descriptor: OciDescriptor::new(
            "application/vnd.oci.image.manifest.v1+json",
            format!("sha256:{}", "f".repeat(64)),
            512,
        )?,
        platforms: vec![BuildPlatform::parse("linux/amd64")?],
        content_bytes: 2048,
        blob_count: 3,
    };
    let key = build_cache_key(&config, &parent, &revision)?;
    let cache = ValidatedBuildCache::new(
        key.clone(),
        runtime_output,
        OciDescriptor::new(
            "application/vnd.oci.image.index.v1+json",
            format!("sha256:{}", "9".repeat(64)),
            256,
        )?,
        1024,
        2,
    )?;
    parent.record_validated_output(output, Some(cache.clone()), Utc::now())?;
    parent.request_cancellation(Utc::now())?;
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
    assert_eq!(build_cache_key(&config, &retry, &revision)?, key);
    let cached_spec = project_task_spec(&config, &retry, &revision, Some(&cache))?;
    let expected_cache_import = format!("type=local,src={CACHE_IMPORT_ROOT}/cache");
    assert!(cached_spec.process.args.windows(2).any(|arguments| {
        arguments[0] == "--import-cache" && arguments[1] == expected_cache_import
    }));
    assert_eq!(
        cached_spec
            .process
            .environment
            .get("A3S_BUILD_CACHE_IMPORT"),
        Some(&"1".into())
    );
    assert!(cached_spec.process.args.first().is_some_and(|script| {
        script.contains(&format!(
            "cp -R {CACHE_ROOT}/cache {CACHE_IMPORT_ROOT}/cache"
        )) && script.contains(&format!("chmod -R u+w {CACHE_IMPORT_ROOT}/cache"))
    }));
    assert_eq!(cached_spec.mounts.len(), 4);
    assert!(matches!(
        &cached_spec.mounts[2].source,
        RuntimeMountSource::Artifact { artifact }
            if artifact.digest == cache.artifact.digest
                && cached_spec.mounts[2].target == CACHE_ROOT
                && cached_spec.mounts[2].read_only
    ));
    assert!(matches!(
        &cached_spec.mounts[3].source,
        RuntimeMountSource::Tmpfs { size_bytes }
            if *size_bytes == cache.artifact.size_bytes
                && cached_spec.mounts[3].target == CACHE_IMPORT_ROOT
                && !cached_spec.mounts[3].read_only
    ));

    let mut changed_config = config.clone();
    changed_config.buildkit_socket_volume_id = "a3s-cloud-buildkit-v0-31-2-replaced".into();
    assert_ne!(
        build_cache_key(&changed_config, &retry, &revision)?,
        cache.key
    );
    assert!(project_task_spec(&changed_config, &retry, &revision, Some(&cache)).is_err());

    let mut other_build = retry;
    let mut other_revision = revision;
    let other_organization = OrganizationId::new();
    other_build.organization_id = other_organization;
    other_revision.organization_id = other_organization;
    assert_ne!(
        build_cache_key(&config, &other_build, &other_revision)?,
        cache.key
    );
    Ok(())
}
