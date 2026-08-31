use super::AgentExecutionFlowRuntime;
use crate::modules::agents::domain::{AgentCodeRunBinding, AgentExecution};
use crate::modules::shared_kernel::domain::{canonical_json_bounded, sha256_digest, Sha256Digest};
use crate::modules::workloads::{project_runtime_spec, ActiveRuntimeTarget, SecretBindingTarget};
use a3s_cloud_contracts::{
    AgentProtocolRunIdentityV1, AgentProviderCapabilityV1, HarnessAgentReleaseBindingV1,
    HarnessInvocationProfileV1, HarnessMcpBindingV1, HarnessProviderBindingV1,
    HarnessSecretReferenceV1, HarnessSecretTargetV1, HarnessSkillBindingV1,
    HarnessWorkspaceBindingV1, RuntimeServiceEndpoint, HARNESS_INVOCATION_PROFILE_MAX_BYTES,
};
use a3s_runtime::contract::{RuntimeUnitClass, TransportProtocol};
use a3s_runtime::RuntimeConsumerRequirements;
use chrono::{DateTime, Utc};

pub(super) async fn ready(
    runtime: &AgentExecutionFlowRuntime,
    execution: &AgentExecution,
    target: &ActiveRuntimeTarget,
    now: DateTime<Utc>,
) -> Result<(AgentCodeRunBinding, u64), String> {
    let provider = runtime
        .providers
        .provider_for_profile(&execution.provider)?;
    let node_id = target
        .replica_binding
        .node_id
        .filter(|node_id| Some(*node_id) == target.deployment.node_id)
        .ok_or_else(|| "Agent Workload has no exact placed Runtime replica".to_owned())?;
    if target.replica_binding.workload_id != target.workload.id
        || target.replica_binding.revision_id != target.revision.id
        || target.replica_binding.deployment_id != target.deployment.id
    {
        return Err("Agent Workload Runtime binding changed its durable identity".into());
    }
    let template = target.revision.resolved_template()?;
    if template.artifact.digest != execution.agent.artifact_digest().as_str() {
        return Err("Agent Workload artifact does not match the execution release".into());
    }
    let service_port_name = template
        .health
        .as_ref()
        .map(|health| health.port_name.clone())
        .ok_or_else(|| "Agent Workload does not declare the Code Harness health port".to_owned())?;
    let spec = project_runtime_spec(&target.revision)?;
    let observation = runtime
        .node_control
        .latest_runtime_observation(node_id, &spec.unit_id, spec.generation)
        .await
        .map_err(|error| format!("could not load Agent Runtime observation: {error}"))?
        .ok_or_else(|| "Agent Runtime has no observation yet".to_owned())?;
    RuntimeConsumerRequirements::new(RuntimeUnitClass::Service)
        .require_health()
        .require_service_lifecycle()
        .require_service_endpoints()
        .accept_observation(&spec, &observation.observation)
        .map_err(|error| error.to_string())?;
    if observation
        .received_at
        .checked_add_signed(runtime.config.heartbeat_timeout)
        .is_none_or(|fresh_until| fresh_until < now)
        || !observation.observation.converges(&spec)
    {
        return Err("Agent Runtime is not recently observed ready".into());
    }
    let endpoint =
        RuntimeServiceEndpoint::from_observation(&observation.observation, &service_port_name)?;
    if endpoint.protocol != TransportProtocol::Tcp {
        return Err("A3S Code Harness Runtime endpoint is not TCP".into());
    }
    let runtime_started_at_ms = observation
        .observation
        .started_at_ms
        .ok_or_else(|| "A3S Code Harness Runtime has no process start time".to_owned())?;
    let spec_digest = Sha256Digest::parse(spec.digest()?)?;
    let invocation_profile =
        harness_invocation_profile(execution, target, &spec, provider.profile())?;
    let binding = AgentCodeRunBinding::new_with_provider(
        provider.profile().clone(),
        node_id,
        target.workload.id,
        target.revision.id,
        target.deployment.id,
        target.replica_binding.replica_id,
        spec.unit_id,
        spec.generation,
        spec_digest,
        service_port_name,
        AgentProtocolRunIdentityV1 {
            schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
            protocol: provider.profile().native_protocol().into(),
            agent_release_identity: execution.agent.artifact_digest().as_str().into(),
            session_id: format!("agent-conversation-{}", execution.conversation_id),
            run_id: format!("agent-execution-{}", execution.id),
        },
        now,
    )?
    .with_invocation_profile(invocation_profile)?;
    Ok((binding, runtime_started_at_ms))
}

fn harness_invocation_profile(
    execution: &AgentExecution,
    target: &ActiveRuntimeTarget,
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
    provider: &crate::modules::agents::domain::AgentProviderProfileBinding,
) -> Result<HarnessInvocationProfileV1, String> {
    let environment_policy = serde_json::json!({
        "process": &spec.process,
        "secretReferences": &spec.secrets,
    });
    let security_policy = serde_json::json!({
        "isolation": &spec.isolation,
        "mounts": &spec.mounts,
        "network": &spec.network,
        "resources": &spec.resources,
        "health": &spec.health,
        "serviceLifecycle": &spec.service_lifecycle,
        "restart": &spec.restart,
    });
    let environment_policy_digest = sha256_digest(&canonical_json_bounded(
        &environment_policy,
        HARNESS_INVOCATION_PROFILE_MAX_BYTES,
        "Harness environment policy",
    )?);
    let security_policy_digest = sha256_digest(&canonical_json_bounded(
        &security_policy,
        HARNESS_INVOCATION_PROFILE_MAX_BYTES,
        "Harness security policy",
    )?);
    let skills = target
        .revision
        .skill_bindings()
        .iter()
        .map(|binding| HarnessSkillBindingV1 {
            asset_id: binding.asset_id().as_uuid(),
            asset_release_id: binding.asset_release_id().as_uuid(),
            artifact_digest: binding.artifact_digest().as_str().into(),
        })
        .collect();
    let mcp_servers = target
        .revision
        .mcp_binding()
        .map(|binding| HarnessMcpBindingV1 {
            asset_id: binding.asset_id().as_uuid(),
            asset_release_id: binding.asset_release_id().as_uuid(),
            profile_digest: binding.profile_digest().as_str().into(),
        })
        .into_iter()
        .collect();
    let mut secrets = target
        .revision
        .resolved_template()?
        .secrets
        .iter()
        .map(|binding| HarnessSecretReferenceV1 {
            name: binding.name.clone(),
            secret_id: binding.secret_id.as_uuid(),
            version: binding.version,
            target: match &binding.target {
                SecretBindingTarget::Environment { variable } => {
                    HarnessSecretTargetV1::Environment {
                        variable: variable.clone(),
                    }
                }
                SecretBindingTarget::File { path, mode } => HarnessSecretTargetV1::File {
                    path: path.clone(),
                    mode: *mode,
                },
                SecretBindingTarget::RegistryCredential => {
                    HarnessSecretTargetV1::RegistryCredential
                }
            },
        })
        .collect::<Vec<_>>();
    secrets.sort_by(|left, right| left.name.cmp(&right.name));
    let profile = HarnessInvocationProfileV1 {
        schema: HarnessInvocationProfileV1::SCHEMA.into(),
        agent: HarnessAgentReleaseBindingV1 {
            organization_id: execution.organization_id.as_uuid(),
            asset_id: execution.agent.asset_id().as_uuid(),
            asset_release_id: execution.agent.asset_release_id().as_uuid(),
            build_run_id: execution.agent.build_run_id().as_uuid(),
            artifact_digest: execution.agent.artifact_digest().as_str().into(),
        },
        provider: HarnessProviderBindingV1 {
            kind: provider.kind().into(),
            revision: provider.revision().into(),
            profile_digest: provider.profile_digest().into(),
            capability_digest: provider.capability_digest().into(),
        },
        // The immutable OCI digest covers the instructions shipped by this
        // exact Agent release; no mutable manifest is copied into the run.
        instructions_digest: execution.agent.artifact_digest().as_str().into(),
        environment_policy_digest,
        security_policy_digest,
        workspace: HarnessWorkspaceBindingV1 {
            workload_id: target.workload.id.as_uuid(),
            workload_revision_id: target.revision.id.as_uuid(),
            runtime_unit_id: spec.unit_id.clone(),
            runtime_generation: spec.generation,
            runtime_spec_digest: spec.digest()?,
            working_directory: spec.process.working_directory.clone(),
        },
        skills,
        mcp_servers,
        models: Vec::new(),
        secrets,
        tools: Vec::new(),
        required_capabilities: vec![
            AgentProviderCapabilityV1::Cancellation,
            AgentProviderCapabilityV1::Cleanup,
            AgentProviderCapabilityV1::EventPages,
        ],
    };
    profile.validate_for(&provider.profile()?)?;
    Ok(profile)
}
