//! The sole Workloads-owned compiler from an admitted revision into the
//! provider-neutral Runtime contract. This is deterministic application
//! policy, not an infrastructure adapter or provider implementation.

use crate::modules::workloads::domain::entities::{
    DeploymentReplicaBinding, DeploymentRuntimeExecutionBinding, ServiceTemplate,
    WorkloadPlacementGroupMemberPlan, WorkloadReplica, WorkloadReplicaLifecycle, WorkloadRevision,
};
use a3s_cloud_contracts::{
    AgentReleaseCacheMode, AgentReleasePersistentDataMode, AgentReleaseWorkspaceMode,
    CloudSecretReference,
};
use a3s_runtime::contract::{
    ArtifactRef, HealthProbe, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy,
    RuntimeHealthCheck, RuntimeMount, RuntimeMountSource, RuntimeNetworkSpec, RuntimePort,
    RuntimeProcessSpec, RuntimeServiceLifecycle, RuntimeUnitClass, RuntimeUnitSpec,
    SecretReference, SecretTarget, TransportProtocol,
};

pub fn project_runtime_spec(revision: &WorkloadRevision) -> Result<RuntimeUnitSpec, String> {
    project_runtime_spec_with_digest(
        revision,
        revision
            .mcp_binding()
            .map(|binding| binding.profile_digest().as_str()),
    )
}

pub fn project_replica_runtime_spec(
    revision: &WorkloadRevision,
    replica: &WorkloadReplica,
) -> Result<RuntimeUnitSpec, String> {
    replica.validate()?;
    if replica.lifecycle == WorkloadReplicaLifecycle::Retired {
        return Err("retired Workload replica has no Runtime projection".into());
    }
    let mut spec = project_runtime_spec(revision)?;
    spec.unit_id = replica.runtime_unit_id(revision)?;
    spec.generation = replica.generation;
    spec.validate()?;
    Ok(spec)
}

pub(crate) fn project_replica_runtime_spec_with_execution(
    revision: &WorkloadRevision,
    replica: &WorkloadReplica,
    execution: Option<&DeploymentRuntimeExecutionBinding>,
) -> Result<RuntimeUnitSpec, String> {
    let spec = project_replica_runtime_spec(revision, replica)?;
    bind_optional_runtime_execution(spec, execution)
}

pub(crate) fn project_bound_runtime_spec(
    revision: &WorkloadRevision,
    binding: &DeploymentReplicaBinding,
) -> Result<RuntimeUnitSpec, String> {
    if binding.workload_id != revision.workload_id
        || binding.revision_id != revision.id
        || binding.replica_generation == 0
        || binding.runtime_generation != binding.replica_generation
        || binding.runtime_unit_id.trim().is_empty()
    {
        return Err("deployment replica binding has an invalid Runtime projection".into());
    }
    let mut spec = project_runtime_spec(revision)?;
    spec.unit_id.clone_from(&binding.runtime_unit_id);
    spec.generation = binding.runtime_generation;
    spec.validate()?;
    Ok(spec)
}

/// Compile an exact bound Unit using the same Workloads projection authority
/// as ordinary deployment. Generic Runtime semantics come only from the
/// Workloads-owned immutable Deployment admission; callers cannot supply them.
pub(crate) fn project_bound_runtime_spec_with_execution(
    revision: &WorkloadRevision,
    binding: &DeploymentReplicaBinding,
    execution: Option<&DeploymentRuntimeExecutionBinding>,
) -> Result<RuntimeUnitSpec, String> {
    let spec = project_bound_runtime_spec(revision, binding)?;
    bind_optional_runtime_execution(spec, execution)
}

pub(crate) fn project_placement_group_runtime_spec_with_execution(
    revision: &WorkloadRevision,
    binding: &DeploymentReplicaBinding,
    plan: &WorkloadPlacementGroupMemberPlan,
    execution: Option<&DeploymentRuntimeExecutionBinding>,
) -> Result<RuntimeUnitSpec, String> {
    let spec = project_placement_group_runtime_spec(revision, binding, plan)?;
    bind_optional_runtime_execution(spec, execution)
}

fn bind_optional_runtime_execution(
    spec: RuntimeUnitSpec,
    execution: Option<&DeploymentRuntimeExecutionBinding>,
) -> Result<RuntimeUnitSpec, String> {
    match execution {
        Some(execution) => {
            execution.validate()?;
            match execution.execution() {
                Some(execution) => bind_runtime_execution(spec, execution),
                None => Ok(spec),
            }
        }
        None => Ok(spec),
    }
}

fn bind_runtime_execution(
    mut spec: RuntimeUnitSpec,
    execution: &crate::modules::workloads::domain::entities::WorkloadRuntimeExecutionBinding,
) -> Result<RuntimeUnitSpec, String> {
    execution.validate()?;
    if spec.class != execution.runtime_class() {
        return Err(
            "Workload revision cannot change Runtime Unit class at identity admission".into(),
        );
    }
    spec.isolation = execution.isolation();
    spec.semantics_profile_digest = Some(execution.semantics_profile_digest().as_str().into());
    spec.identity_attachment_digest = Some(execution.identity_attachment_digest().as_str().into());
    spec.validate()?;
    Ok(spec)
}

pub(crate) fn project_placement_group_runtime_spec(
    revision: &WorkloadRevision,
    binding: &DeploymentReplicaBinding,
    plan: &WorkloadPlacementGroupMemberPlan,
) -> Result<RuntimeUnitSpec, String> {
    if binding.workload_id != revision.workload_id
        || binding.revision_id != revision.id
        || binding.member_id != plan.member_id
        || binding.runtime_unit_id != plan.runtime_unit_id
        || binding.runtime_generation != binding.replica_generation
        || binding.runtime_generation == 0
        || plan.template.digest()? != plan.template_digest
    {
        return Err("placement-group member has an invalid Runtime projection".into());
    }
    let mut spec = project_runtime_spec_from_template(
        revision,
        &plan.template,
        revision
            .mcp_binding()
            .map(|binding| binding.profile_digest().as_str()),
    )?;
    spec.unit_id.clone_from(&binding.runtime_unit_id);
    spec.generation = binding.runtime_generation;
    spec.validate()?;
    Ok(spec)
}

/// Project one ordinary Runtime Service while binding an optional immutable
/// product semantics profile. Runtime treats the digest as opaque.
pub(crate) fn project_runtime_spec_with_digest(
    revision: &WorkloadRevision,
    semantics_profile_digest: Option<&str>,
) -> Result<RuntimeUnitSpec, String> {
    let template = revision.resolved_template()?;
    project_runtime_spec_from_template(revision, template, semantics_profile_digest)
}

fn project_runtime_spec_from_template(
    revision: &WorkloadRevision,
    template: &ServiceTemplate,
    semantics_profile_digest: Option<&str>,
) -> Result<RuntimeUnitSpec, String> {
    let health = template.health.as_ref().map(project_readiness);
    let service_lifecycle = revision
        .agent_binding()
        .map(|binding| -> Result<RuntimeServiceLifecycle, String> {
            let manifest = binding.runtime_manifest_for(template)?;
            let readiness = template.health.as_ref().ok_or_else(|| {
                "Agent Service template omitted its manifest-owned readiness policy".to_owned()
            })?;
            Ok(RuntimeServiceLifecycle {
                liveness: RuntimeHealthCheck {
                    probe: HealthProbe::Http {
                        port: readiness.port_name.clone(),
                        path: manifest.health().liveness_path().into(),
                        expected_statuses: vec![200],
                    },
                    interval_ms: readiness.interval_ms,
                    timeout_ms: readiness.timeout_ms,
                    start_period_ms: readiness.stabilization_window_ms,
                    success_threshold: u32::from(readiness.healthy_threshold),
                    failure_threshold: u32::from(readiness.unhealthy_threshold),
                },
                shutdown_grace_seconds: manifest.health().shutdown_grace_seconds(),
            })
        })
        .transpose()?;
    let spec = RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id: revision.runtime_unit_id(),
        generation: revision.generation,
        class: RuntimeUnitClass::Service,
        artifact: ArtifactRef {
            uri: template.artifact.uri.clone(),
            digest: template.artifact.digest.clone(),
            media_type: template.artifact.media_type.clone(),
        },
        process: RuntimeProcessSpec {
            command: template.process.command.clone(),
            args: template.process.args.clone(),
            working_directory: template.process.working_directory.clone(),
            environment: template.process.environment.clone(),
        },
        mounts: project_runtime_mounts(revision, template)?,
        secrets: project_runtime_secrets(revision)?,
        network: RuntimeNetworkSpec {
            mode: if template.ports.is_empty() {
                NetworkMode::None
            } else {
                NetworkMode::Service
            },
            ports: template
                .ports
                .iter()
                .map(|port| RuntimePort {
                    name: port.name.clone(),
                    container_port: port.container_port,
                    protocol: TransportProtocol::Tcp,
                })
                .collect(),
        },
        resources: ResourceLimits {
            cpu_millis: template.resources.cpu_millis,
            memory_bytes: template.resources.memory_bytes,
            pids: template.resources.pids,
            ephemeral_storage_bytes: template.resources.ephemeral_storage_bytes,
            execution_timeout_ms: None,
        },
        isolation: IsolationLevel::Sandbox,
        health,
        service_lifecycle,
        restart: RestartPolicy::Always,
        outputs: Vec::new(),
        semantics_profile_digest: semantics_profile_digest.map(str::to_owned),
        identity_attachment_digest: None,
    };
    spec.validate()?;
    Ok(spec)
}

fn project_readiness(
    health: &crate::modules::workloads::domain::entities::HttpHealthCheck,
) -> RuntimeHealthCheck {
    RuntimeHealthCheck {
        probe: HealthProbe::Http {
            port: health.port_name.clone(),
            path: health.path.clone(),
            expected_statuses: vec![200],
        },
        interval_ms: health.interval_ms,
        timeout_ms: health.timeout_ms,
        start_period_ms: health.stabilization_window_ms,
        success_threshold: u32::from(health.healthy_threshold),
        failure_threshold: u32::from(health.unhealthy_threshold),
    }
}

fn project_runtime_mounts(
    revision: &WorkloadRevision,
    template: &ServiceTemplate,
) -> Result<Vec<RuntimeMount>, String> {
    let mut mounts = revision
        .skill_bindings()
        .iter()
        .map(|binding| {
            Ok(RuntimeMount {
                name: binding.mount_name(),
                source: RuntimeMountSource::Artifact {
                    artifact: ArtifactRef {
                        uri: binding.artifact_uri()?,
                        digest: binding.artifact_digest().as_str().into(),
                        media_type: binding.artifact_media_type().into(),
                    },
                },
                target: binding.mount_target(),
                read_only: true,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let Some(binding) = revision.agent_binding() else {
        return Ok(mounts);
    };
    let contract = binding.runtime_contract().ok_or_else(|| {
        "Agent Workload binding predates the exact release manifest contract".to_owned()
    })?;
    let manifest = contract.manifest()?;
    mounts.push(RuntimeMount {
        name: "agent-release-manifest".into(),
        source: RuntimeMountSource::Artifact {
            artifact: ArtifactRef {
                uri: contract.archive_uri().into(),
                digest: contract.archive_digest().to_string(),
                media_type: a3s_cloud_contracts::NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
            },
        },
        target: "/app/.a3s".into(),
        read_only: true,
    });
    let storage_bytes = template
        .resources
        .ephemeral_storage_bytes
        .ok_or_else(|| "Agent Workload omitted its ephemeral storage limit".to_owned())?;
    let workspace_read_only = match manifest.storage().workspace() {
        AgentReleaseWorkspaceMode::ReadOnly => true,
        AgentReleaseWorkspaceMode::Ephemeral => false,
        _ => return Err("Agent release workspace mode is unsupported".into()),
    };
    mounts.push(RuntimeMount {
        name: "agent-workspace".into(),
        source: RuntimeMountSource::Tmpfs {
            size_bytes: storage_bytes,
        },
        target: "/workspace".into(),
        read_only: workspace_read_only,
    });
    match manifest.storage().cache() {
        AgentReleaseCacheMode::None => {}
        AgentReleaseCacheMode::Ephemeral if workspace_read_only => mounts.push(RuntimeMount {
            name: "agent-cache".into(),
            source: RuntimeMountSource::Tmpfs {
                size_bytes: storage_bytes,
            },
            target: "/workspace/.cache".into(),
            read_only: false,
        }),
        AgentReleaseCacheMode::Ephemeral => {}
        _ => return Err("Agent release cache mode is unsupported".into()),
    }
    match manifest.storage().persistent_data() {
        AgentReleasePersistentDataMode::None => {}
        AgentReleasePersistentDataMode::External => {
            return Err(
                "Agent release external persistent data has no versioned mount target".into(),
            )
        }
        _ => return Err("Agent release persistent-data mode is unsupported".into()),
    }
    Ok(mounts)
}

/// Project the sole Workloads-owned Secret bindings into exact Runtime
/// references. Internal owner-composed Tasks reuse this function so they
/// cannot drift into a second Secret-reference format or subject identity.
pub(crate) fn project_runtime_secrets(
    revision: &WorkloadRevision,
) -> Result<Vec<SecretReference>, String> {
    let template = revision.resolved_template()?;
    template
        .secrets
        .iter()
        .map(|binding| {
            let reference = CloudSecretReference::new(
                revision.id.as_uuid(),
                binding.secret_id.as_uuid(),
                binding.version,
            )?;
            let target = match &binding.target {
                crate::modules::workloads::domain::entities::SecretBindingTarget::Environment {
                    variable,
                } => SecretTarget::Environment {
                    variable: variable.clone(),
                },
                crate::modules::workloads::domain::entities::SecretBindingTarget::File {
                    path,
                    mode,
                } => SecretTarget::File {
                    path: path.clone(),
                    mode: *mode,
                },
                crate::modules::workloads::domain::entities::SecretBindingTarget::RegistryCredential => {
                    SecretTarget::RegistryCredential
                }
            };
            Ok(SecretReference {
                name: binding.name.clone(),
                reference: reference.to_string(),
                target,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "runtime_projection/tests.rs"]
mod tests;
