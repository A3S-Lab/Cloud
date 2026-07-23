use super::BuildFlowConfig;
use crate::modules::artifacts::domain::{BuildRun, BuildRunStatus};
use crate::modules::sources::domain::ExternalSourceRevision;
use a3s_cloud_contracts::{validate_cloud_artifact, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE};
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy, RuntimeMount,
    RuntimeMountSource, RuntimeNetworkSpec, RuntimeOutputSpec, RuntimeProcessSpec,
    RuntimeUnitClass, RuntimeUnitSpec,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(super) const SOURCE_ROOT: &str = "/home/user/a3s-source";
pub(super) const OUTPUT_ROOT: &str = "/home/user/a3s-output";
pub(super) const BUILDKIT_SOCKET_ROOT: &str = "/run/user/1000/a3s-buildkit";
pub(super) const BUILDKIT_ADDRESS: &str = "unix:///run/user/1000/a3s-buildkit/buildkitd.sock";
pub(super) const OUTPUT_NAME: &str = "oci-layout";

const TASK_SCRIPT: &str =
    "umask 077\nmkdir -p /home/user/a3s-output /home/user/a3s-home\nexec /usr/bin/buildctl \"$@\"";
const SEMANTICS_PROFILE: &str = "a3s.cloud.buildkit-runtime-task.v1:network-none";

pub(super) fn project_task_spec(
    config: &BuildFlowConfig,
    build: &BuildRun,
    revision: &ExternalSourceRevision,
) -> Result<RuntimeUnitSpec, String> {
    if build.organization_id != revision.organization_id
        || build.project_id != revision.project_id
        || build.environment_id != revision.environment_id
        || build.source_revision_id != revision.id
        || !matches!(
            build.status,
            BuildRunStatus::Prepared
                | BuildRunStatus::Scheduled
                | BuildRunStatus::Running
                | BuildRunStatus::Validating
                | BuildRunStatus::Publishing
                | BuildRunStatus::Cancelling
                | BuildRunStatus::CleanupPending
                | BuildRunStatus::Succeeded
                | BuildRunStatus::Failed
                | BuildRunStatus::Cancelled
        )
    {
        return Err("build Task projection does not match durable build identity".into());
    }
    let input = build
        .input_artifact
        .as_ref()
        .ok_or_else(|| "build Task projection requires a prepared input Artifact".to_owned())?;
    let input = ArtifactRef {
        uri: input.uri.clone(),
        digest: input.digest.clone(),
        media_type: input.media_type.clone(),
    };
    validate_cloud_artifact(&input)?;
    let mut environment = BTreeMap::new();
    environment.insert("HOME".into(), "/home/user/a3s-home".into());
    environment.insert("XDG_CONFIG_HOME".into(), "/home/user/a3s-home".into());
    environment.insert("DOCKER_CONFIG".into(), "/home/user/a3s-home".into());
    environment.insert("LC_ALL".into(), "C".into());
    let spec = RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id: build.runtime_unit_id(),
        generation: BuildRun::RUNTIME_GENERATION,
        class: RuntimeUnitClass::Task,
        artifact: config.builder.clone(),
        process: RuntimeProcessSpec {
            command: vec!["/bin/sh".into(), "-ceu".into()],
            args: buildctl_arguments(revision),
            working_directory: Some("/home/user".into()),
            environment,
        },
        mounts: vec![
            RuntimeMount {
                name: "source".into(),
                source: RuntimeMountSource::Artifact { artifact: input },
                target: SOURCE_ROOT.into(),
                read_only: true,
            },
            RuntimeMount {
                name: "buildkit-socket".into(),
                source: RuntimeMountSource::Volume {
                    volume_id: config.buildkit_socket_volume_id.clone(),
                },
                target: BUILDKIT_SOCKET_ROOT.into(),
                read_only: true,
            },
        ],
        secrets: Vec::new(),
        network: RuntimeNetworkSpec {
            mode: NetworkMode::None,
            ports: Vec::new(),
        },
        resources: ResourceLimits {
            cpu_millis: config.cpu_millis,
            memory_bytes: config.memory_bytes,
            pids: config.pids,
            ephemeral_storage_bytes: None,
            execution_timeout_ms: Some(
                u64::try_from(config.execution_timeout.num_milliseconds())
                    .map_err(|_| "build Task execution timeout is invalid")?,
            ),
        },
        isolation: IsolationLevel::Container,
        health: None,
        restart: RestartPolicy::Never,
        outputs: vec![RuntimeOutputSpec {
            name: OUTPUT_NAME.into(),
            path: OUTPUT_ROOT.into(),
            media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
            max_bytes: config.output_max_bytes,
        }],
        semantics_profile_digest: Some(format!(
            "sha256:{:x}",
            Sha256::digest(SEMANTICS_PROFILE.as_bytes())
        )),
    };
    spec.validate()?;
    Ok(spec)
}

fn buildctl_arguments(revision: &ExternalSourceRevision) -> Vec<String> {
    let recipe = &revision.recipe;
    let context = if recipe.context_path() == "." {
        SOURCE_ROOT.to_owned()
    } else {
        format!("{SOURCE_ROOT}/{}", recipe.context_path())
    };
    let mut arguments = vec![
        TASK_SCRIPT.into(),
        "a3s-buildctl".into(),
        "--addr".into(),
        BUILDKIT_ADDRESS.into(),
        "build".into(),
        "--frontend".into(),
        "dockerfile.v0".into(),
        "--progress".into(),
        "plain".into(),
        "--local".into(),
        format!("context={context}"),
        "--local".into(),
        format!("dockerfile={SOURCE_ROOT}"),
        "--opt".into(),
        format!("filename={}", recipe.dockerfile_path()),
        "--opt".into(),
        format!(
            "platform={}",
            recipe
                .platforms()
                .iter()
                .map(|platform| platform.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ),
        "--opt".into(),
        "force-network-mode=none".into(),
    ];
    if let Some(target) = recipe.target() {
        arguments.extend(["--opt".into(), format!("target={target}")]);
    }
    arguments.extend([
        "--output".into(),
        format!("type=oci,dest={OUTPUT_ROOT}/oci,tar=false,oci-mediatypes=true"),
        "--metadata-file".into(),
        format!("{OUTPUT_ROOT}/buildkit-metadata.json"),
    ]);
    arguments
}
