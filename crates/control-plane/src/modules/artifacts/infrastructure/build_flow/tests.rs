use super::task_spec::{project_task_spec, BUILDKIT_ADDRESS, OUTPUT_NAME};
use super::BuildFlowConfig;
use crate::modules::artifacts::domain::{BuildArtifact, BuildRun};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, SourceRevisionId,
};
use crate::modules::sources::domain::{
    BuildRecipe, ExternalSourceRevision, GitCommitSha, GitProvider, GitRepository,
    NewExternalSourceRevision,
};
use a3s_cloud_contracts::NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE;
use a3s_runtime::contract::{ArtifactRef, NetworkMode, RuntimeMountSource, RuntimeUnitClass};
use chrono::Utc;

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

    let spec = project_task_spec(&config, &build, &revision)?;
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
    assert!(!spec.process.args.iter().any(|argument| {
        matches!(
            argument.as_str(),
            "--allow" | "--ssh" | "--secret" | "--import-cache" | "--export-cache"
        )
    }));
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
    Ok(())
}
