use crate::modules::durable_cells::domain::{
    DurableCellProviderBinding, DurableCellServiceProfile,
};
use crate::modules::workloads::infrastructure::runtime_spec::project_runtime_spec_with_digest;
use crate::modules::workloads::WorkloadRevision;
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandEnvelope, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
};
use a3s_runtime::contract::{
    HealthProbe, NetworkMode, RuntimeHealthState, RuntimeObservation, RuntimeServiceEndpoint,
    RuntimeUnitClass, RuntimeUnitSpec, TransportProtocol,
};

/// The two existing Runtime endpoints admitted from one exact Fleet command
/// acknowledgement. This is a bounded view, not an endpoint registry or
/// provider lifecycle record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellRuntimeEndpoints {
    pub public: RuntimeServiceEndpoint,
    pub internal: RuntimeServiceEndpoint,
}

pub fn project_durable_cell_runtime_spec(
    binding: &DurableCellProviderBinding,
    service_profile: &DurableCellServiceProfile,
    workload_revision: &WorkloadRevision,
) -> Result<RuntimeUnitSpec, String> {
    binding.validate_workload_revision(service_profile, workload_revision)?;
    let spec = project_runtime_spec_with_digest(
        workload_revision,
        Some(binding.service_profile_digest.as_str()),
    )?;
    validate_runtime_spec(binding, service_profile, workload_revision, &spec)?;
    Ok(spec)
}

/// Admit an existing Fleet `RuntimeApply` acknowledgement as a healthy Cell
/// provider replica. Fleet remains the only command journal and Runtime remains
/// the only Service observation/endpoint authority.
pub fn admit_durable_cell_runtime_apply(
    binding: &DurableCellProviderBinding,
    service_profile: &DurableCellServiceProfile,
    workload_revision: &WorkloadRevision,
    command: &NodeCommandEnvelope,
    acknowledgement: &NodeCommandAck,
) -> Result<DurableCellRuntimeEndpoints, String> {
    let expected = project_durable_cell_runtime_spec(binding, service_profile, workload_revision)?;
    command.validate()?;
    acknowledgement.validate_against(command)?;
    if acknowledgement.schema != NodeCommandAck::SCHEMA {
        return Err(
            "Durable Cell Runtime admission requires the current Fleet receipt schema".into(),
        );
    }
    let NodeCommandPayload::RuntimeApply { request, .. } = &command.payload else {
        return Err("Durable Cell Runtime admission requires a Fleet RuntimeApply command".into());
    };
    if request.spec != expected {
        return Err(
            "Durable Cell RuntimeApply command changed the exact provider Service projection"
                .into(),
        );
    }
    let NodeCommandOutcome::Succeeded { result } = &acknowledgement.outcome else {
        return Err("Durable Cell provider RuntimeApply did not succeed".into());
    };
    let NodeCommandResult::RuntimeApplied { observation } = result.as_ref() else {
        return Err("Durable Cell provider receipt is not a RuntimeApply result".into());
    };
    admit_running_observation(binding, service_profile, &expected, observation)
}

fn validate_runtime_spec(
    binding: &DurableCellProviderBinding,
    profile: &DurableCellServiceProfile,
    revision: &WorkloadRevision,
    spec: &RuntimeUnitSpec,
) -> Result<(), String> {
    binding.validate_workload_revision(profile, revision)?;
    spec.validate()?;
    if spec.unit_id != revision.runtime_unit_id()
        || spec.generation != binding.workload_generation
        || spec.class != RuntimeUnitClass::Service
        || spec.artifact.digest != binding.provider_artifact_digest.as_str()
        || spec.semantics_profile_digest.as_deref() != Some(binding.service_profile_digest.as_str())
        || spec.network.mode != NetworkMode::Service
        || spec.network.ports.len() != 2
        || spec
            .network
            .ports
            .iter()
            .any(|port| port.protocol != TransportProtocol::Tcp)
    {
        return Err(
            "Durable Cell provider did not project as the exact ordinary Runtime Service".into(),
        );
    }
    let profile = profile.spec();
    if !spec
        .network
        .ports
        .iter()
        .any(|port| port.name == profile.public_runtime_port)
        || !spec
            .network
            .ports
            .iter()
            .any(|port| port.name == profile.internal_runtime_port)
    {
        return Err("Durable Cell Runtime Service changed its profile ports".into());
    }
    let Some(health) = &spec.health else {
        return Err("Durable Cell Runtime Service omitted its health probe".into());
    };
    match &health.probe {
        HealthProbe::Http {
            port,
            path,
            expected_statuses,
        } if port == &profile.internal_runtime_port
            && path == &profile.health_path
            && expected_statuses.as_slice() == [200] =>
        {
            Ok(())
        }
        _ => Err("Durable Cell Runtime Service changed its internal HTTP health probe".into()),
    }
}

fn admit_running_observation(
    binding: &DurableCellProviderBinding,
    profile: &DurableCellServiceProfile,
    spec: &RuntimeUnitSpec,
    observation: &RuntimeObservation,
) -> Result<DurableCellRuntimeEndpoints, String> {
    observation.validate_against(spec)?;
    if !observation.converges(spec)
        || observation
            .health
            .as_ref()
            .is_none_or(|health| health.state != RuntimeHealthState::Healthy)
        || observation.evidence.as_ref().is_none_or(|evidence| {
            evidence.semantics_profile_digest.as_deref()
                != Some(binding.service_profile_digest.as_str())
        })
    {
        return Err(
            "Durable Cell provider Runtime observation is not exact, running, and healthy".into(),
        );
    }
    let public =
        RuntimeServiceEndpoint::from_observation(observation, &profile.spec().public_runtime_port)?;
    let internal = RuntimeServiceEndpoint::from_observation(
        observation,
        &profile.spec().internal_runtime_port,
    )?;
    if public.protocol != TransportProtocol::Tcp
        || internal.protocol != TransportProtocol::Tcp
        || public.socket_addr() == internal.socket_addr()
    {
        return Err("Durable Cell Runtime endpoints are not distinct TCP sockets".into());
    }
    Ok(DurableCellRuntimeEndpoints { public, internal })
}
