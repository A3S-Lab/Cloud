use super::BuildFlowConfig;
use crate::modules::artifacts::domain::{
    BuildRun, BuildRunStatus, ValidatedBuildCache, BUILD_CACHE_SCHEMA,
};
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
pub(super) const CACHE_ROOT: &str = "/home/user/a3s-cache";
pub(super) const CACHE_IMPORT_ROOT: &str = "/home/user/a3s-cache-import";
pub(super) const BUILDKIT_SOCKET_ROOT: &str = "/run/user/1000/a3s-buildkit";
pub(super) const BUILDKIT_ADDRESS: &str = "unix:///run/user/1000/a3s-buildkit/buildkitd.sock";
pub(super) const OUTPUT_NAME: &str = "oci-layout";

const LEGACY_TASK_SCRIPT: &str =
    "umask 077\nmkdir -p /home/user/a3s-output /home/user/a3s-home\nexec /usr/bin/buildctl \"$@\"";
const CACHE_TASK_SCRIPT: &str = concat!(
    "umask 077\n",
    "mkdir -p /home/user/a3s-output /home/user/a3s-home\n",
    "printf '{\"schema\":\"",
    "a3s.cloud.build-cache.v1",
    "\",\"key\":\"%s\"}\\n' \"$A3S_BUILD_CACHE_KEY\" ",
    "> /home/user/a3s-output/build-cache.json\n",
    "if [ \"${A3S_BUILD_CACHE_IMPORT:-0}\" = 1 ]; then\n",
    "  test -d /home/user/a3s-cache/cache || { ",
    "echo A3S_BUILD_CACHE_IMPORT_SOURCE_MISSING >&2; exit 51; }\n",
    "  mkdir -p /home/user/a3s-cache-import || { ",
    "echo A3S_BUILD_CACHE_IMPORT_STAGING_CREATE_FAILED >&2; exit 52; }\n",
    "  cp -R /home/user/a3s-cache/cache /home/user/a3s-cache-import/cache || { ",
    "echo A3S_BUILD_CACHE_IMPORT_COPY_FAILED >&2; exit 53; }\n",
    "  chmod -R u+w /home/user/a3s-cache-import/cache || { ",
    "echo A3S_BUILD_CACHE_IMPORT_PERMISSIONS_FAILED >&2; exit 54; }\n",
    "fi\n",
    "exec /usr/bin/buildctl \"$@\"",
);
const LEGACY_SEMANTICS_PROFILE: &str = "a3s.cloud.buildkit-runtime-task.v1:network-none";
const CACHE_SEMANTICS_PROFILE: &str =
    "a3s.cloud.buildkit-runtime-task.v3:network-none:readonly-cache-staging";
const CACHE_KEY_PROFILE: &str = "a3s.cloud.build-cache-key.v1";

pub(super) fn project_task_spec(
    config: &BuildFlowConfig,
    build: &BuildRun,
    revision: &ExternalSourceRevision,
    cache: Option<&ValidatedBuildCache>,
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
                | BuildRunStatus::Attesting
                | BuildRunStatus::Cancelling
                | BuildRunStatus::CleanupPending
                | BuildRunStatus::Succeeded
                | BuildRunStatus::Failed
                | BuildRunStatus::Cancelled
        )
    {
        return Err("build Task projection does not match durable build identity".into());
    }
    if !build.cache_required && cache.is_some() {
        return Err("legacy build Task cannot import a content-addressed cache".into());
    }
    if cache.is_some() && build.retry_of_build_run_id.is_none() {
        return Err("an initial build Task cannot import a prior build cache".into());
    }
    let expected_cache_key = build_cache_key(config, build, revision)?;
    if let Some(cache) = cache {
        cache.validate()?;
        if cache.key != expected_cache_key {
            return Err("build cache key does not match immutable build inputs".into());
        }
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
    if build.cache_required {
        environment.insert("A3S_BUILD_CACHE_KEY".into(), expected_cache_key);
    }
    if cache.is_some() {
        environment.insert("A3S_BUILD_CACHE_IMPORT".into(), "1".into());
    }
    let mut mounts = vec![
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
    ];
    if let Some(cache) = cache {
        mounts.push(RuntimeMount {
            name: "build-cache".into(),
            source: RuntimeMountSource::Artifact {
                artifact: ArtifactRef {
                    uri: cache.artifact.uri.clone(),
                    digest: cache.artifact.digest.clone(),
                    media_type: cache.artifact.media_type.clone(),
                },
            },
            target: CACHE_ROOT.into(),
            read_only: true,
        });
        mounts.push(RuntimeMount {
            name: "build-cache-import".into(),
            source: RuntimeMountSource::Tmpfs {
                size_bytes: cache.artifact.size_bytes,
            },
            target: CACHE_IMPORT_ROOT.into(),
            read_only: false,
        });
    }
    let semantics_profile = if build.cache_required {
        CACHE_SEMANTICS_PROFILE
    } else {
        LEGACY_SEMANTICS_PROFILE
    };
    let spec = RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id: build.runtime_unit_id(),
        generation: BuildRun::RUNTIME_GENERATION,
        class: RuntimeUnitClass::Task,
        artifact: config.builder.clone(),
        process: RuntimeProcessSpec {
            command: vec!["/bin/sh".into(), "-ceu".into()],
            args: buildctl_arguments(revision, build.cache_required, cache.is_some()),
            working_directory: Some("/home/user".into()),
            environment,
        },
        mounts,
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
            Sha256::digest(semantics_profile.as_bytes())
        )),
    };
    spec.validate()?;
    Ok(spec)
}

pub(super) fn build_cache_key(
    config: &BuildFlowConfig,
    build: &BuildRun,
    revision: &ExternalSourceRevision,
) -> Result<String, String> {
    if build.organization_id != revision.organization_id
        || build.project_id != revision.project_id
        || build.environment_id != revision.environment_id
        || build.source_revision_id != revision.id
    {
        return Err("build cache identity does not match source ownership".into());
    }
    let source_digest = build
        .source_content_digest
        .as_deref()
        .ok_or_else(|| "build cache identity requires a prepared source digest".to_owned())?;
    let mut digest = Sha256::new();
    for value in [
        CACHE_KEY_PROFILE,
        BUILD_CACHE_SCHEMA,
        &build.organization_id.to_string(),
        &build.project_id.to_string(),
        &build.environment_id.to_string(),
        source_digest,
        revision.recipe_digest.as_str(),
        config.builder.uri.as_str(),
        config.builder.digest.as_str(),
        config.builder.media_type.as_str(),
        config.buildkit_socket_volume_id.as_str(),
        CACHE_SEMANTICS_PROFILE,
    ] {
        let length = u64::try_from(value.len())
            .map_err(|_| "build cache identity field exceeds its bound")?;
        digest.update(length.to_be_bytes());
        digest.update(value.as_bytes());
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn buildctl_arguments(
    revision: &ExternalSourceRevision,
    cache_required: bool,
    cache_available: bool,
) -> Vec<String> {
    let recipe = &revision.recipe;
    let context = if recipe.context_path() == "." {
        SOURCE_ROOT.to_owned()
    } else {
        format!("{SOURCE_ROOT}/{}", recipe.context_path())
    };
    let mut arguments = vec![
        if cache_required {
            CACHE_TASK_SCRIPT.into()
        } else {
            LEGACY_TASK_SCRIPT.into()
        },
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
    if cache_available {
        arguments.extend([
            "--import-cache".into(),
            format!("type=local,src={CACHE_IMPORT_ROOT}/cache"),
        ]);
    }
    arguments.extend([
        "--output".into(),
        format!("type=oci,dest={OUTPUT_ROOT}/oci,tar=false,oci-mediatypes=true"),
        "--metadata-file".into(),
        format!("{OUTPUT_ROOT}/buildkit-metadata.json"),
    ]);
    if cache_required {
        arguments.extend([
            "--export-cache".into(),
            format!(
                "type=local,dest={OUTPUT_ROOT}/cache,mode=max,oci-mediatypes=true,image-manifest=true"
            ),
        ]);
    }
    arguments
}
