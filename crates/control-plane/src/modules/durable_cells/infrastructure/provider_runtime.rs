use crate::modules::durable_cells::domain::{
    DurableCellProviderBinding, DurableCellServiceProfile,
};
use crate::modules::workloads::infrastructure::runtime_spec::project_runtime_spec_with_digest;
use crate::modules::workloads::WorkloadRevision;
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandEnvelope, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
    NodeDurableCellOperatorBindingV1, NodeDurableCellOperatorObservationV1,
};
use a3s_runtime::contract::{
    HealthProbe, NetworkMode, RuntimeHealthState, RuntimeInspection, RuntimeObservation,
    RuntimeServiceEndpoint, RuntimeUnitClass, RuntimeUnitSpec, RuntimeUnitState, TransportProtocol,
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

/// Bind the Cell provider's node-local operator port to the same exact
/// application, Workload revision, artifact, and ordinary Runtime Service.
/// The returned value is a Fleet command input, not provider configuration or
/// another deployment record.
pub fn project_durable_cell_operator_binding(
    binding: &DurableCellProviderBinding,
    service_profile: &DurableCellServiceProfile,
    workload_revision: &WorkloadRevision,
) -> Result<NodeDurableCellOperatorBindingV1, String> {
    let spec = project_durable_cell_runtime_spec(binding, service_profile, workload_revision)?;
    let operator = NodeDurableCellOperatorBindingV1 {
        schema: NodeDurableCellOperatorBindingV1::SCHEMA.into(),
        application_id: binding.application_id.as_uuid(),
        application_revision_id: binding.application_revision_id.as_uuid(),
        application_revision_number: binding.application_revision_number,
        workload_id: binding.workload_id.as_uuid(),
        workload_revision_id: binding.workload_revision_id.as_uuid(),
        runtime_unit_id: spec.unit_id.clone(),
        runtime_generation: spec.generation,
        runtime_spec_digest: spec.digest()?,
        service_profile_digest: binding.service_profile_digest.to_string(),
        service_template_digest: binding.service_template_digest.to_string(),
        provider_artifact_digest: binding.provider_artifact_digest.to_string(),
        internal_service_port_name: service_profile.spec().internal_runtime_port.clone(),
    };
    operator.validate()?;
    Ok(operator)
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

/// Admit a sanitized operator observation only after the same exact healthy
/// RuntimeApply receipt is still admissible. This is the adoption proof for
/// the provider generation; it creates no Cell ownership or deployment state.
#[allow(clippy::too_many_arguments)]
pub fn admit_durable_cell_operator_observation(
    binding: &DurableCellProviderBinding,
    service_profile: &DurableCellServiceProfile,
    workload_revision: &WorkloadRevision,
    runtime_apply_command: &NodeCommandEnvelope,
    runtime_apply_acknowledgement: &NodeCommandAck,
    operator_command: &NodeCommandEnvelope,
    operator_acknowledgement: &NodeCommandAck,
) -> Result<NodeDurableCellOperatorObservationV1, String> {
    admit_durable_cell_runtime_apply(
        binding,
        service_profile,
        workload_revision,
        runtime_apply_command,
        runtime_apply_acknowledgement,
    )?;
    let expected =
        project_durable_cell_operator_binding(binding, service_profile, workload_revision)?;
    operator_command.validate()?;
    operator_acknowledgement.validate_against(operator_command)?;
    if operator_acknowledgement.schema != NodeCommandAck::SCHEMA {
        return Err(
            "Durable Cell operator admission requires the current Fleet receipt schema".into(),
        );
    }
    if runtime_apply_command.node_id != operator_command.node_id
        || operator_command.issued_at < runtime_apply_acknowledgement.completed_at
    {
        return Err(
            "Durable Cell operator observation does not follow the same node's healthy apply"
                .into(),
        );
    }
    let NodeCommandPayload::DurableCellOperatorObserve {
        binding: observed_binding,
    } = &operator_command.payload
    else {
        return Err(
            "Durable Cell operator admission requires a Fleet operator observation command".into(),
        );
    };
    if observed_binding.as_ref() != &expected {
        return Err("Durable Cell operator command changed the exact provider binding".into());
    }
    let NodeCommandOutcome::Succeeded { result } = &operator_acknowledgement.outcome else {
        return Err("Durable Cell operator observation did not succeed".into());
    };
    let NodeCommandResult::DurableCellOperatorObserved { observation } = result.as_ref() else {
        return Err("Durable Cell operator receipt changed its result kind".into());
    };
    observation.validate_for(&expected)?;
    Ok(observation.clone())
}

/// Admit graceful drain evidence from Runtime's existing stop operation. The
/// Cell provider receives the same SIGTERM-driven lifecycle as every ordinary
/// Service; no provider-specific shutdown command is introduced.
pub fn admit_durable_cell_runtime_stop(
    binding: &DurableCellProviderBinding,
    service_profile: &DurableCellServiceProfile,
    workload_revision: &WorkloadRevision,
    command: &NodeCommandEnvelope,
    acknowledgement: &NodeCommandAck,
) -> Result<(), String> {
    let expected = project_durable_cell_runtime_spec(binding, service_profile, workload_revision)?;
    validate_current_receipt(command, acknowledgement, "RuntimeStop")?;
    let NodeCommandPayload::RuntimeStop { request } = &command.payload else {
        return Err("Durable Cell drain requires the existing Fleet RuntimeStop command".into());
    };
    if request.unit_id != expected.unit_id || request.generation != expected.generation {
        return Err("Durable Cell RuntimeStop changed the exact provider generation".into());
    }
    let NodeCommandOutcome::Succeeded { result } = &acknowledgement.outcome else {
        return Err("Durable Cell RuntimeStop did not succeed".into());
    };
    let NodeCommandResult::RuntimeStopped { inspection } = result.as_ref() else {
        return Err("Durable Cell drain receipt is not a RuntimeStop result".into());
    };
    match inspection {
        RuntimeInspection::Found { observation, .. } => {
            observation.validate_against(&expected)?;
            if observation.state != RuntimeUnitState::Stopped {
                return Err(
                    "Durable Cell RuntimeStop did not fence the provider generation".into(),
                );
            }
            validate_runtime_evidence_time(
                "stop",
                observation.observed_at_ms,
                command,
                acknowledgement,
            )
        }
        RuntimeInspection::NotFound {
            unit_id,
            last_generation,
            ..
        } if unit_id == &expected.unit_id
            && last_generation.is_none_or(|generation| generation >= expected.generation) =>
        {
            Ok(())
        }
        RuntimeInspection::NotFound { .. } => {
            Err("Durable Cell RuntimeStop absence evidence is stale".into())
        }
    }
}

/// Admit cleanup only from Runtime's existing exact-generation removal
/// receipt. Provider storage deletion remains an S0 lifecycle and is not
/// inferred from this process cleanup evidence.
pub fn admit_durable_cell_runtime_remove(
    binding: &DurableCellProviderBinding,
    service_profile: &DurableCellServiceProfile,
    workload_revision: &WorkloadRevision,
    command: &NodeCommandEnvelope,
    acknowledgement: &NodeCommandAck,
) -> Result<(), String> {
    let expected = project_durable_cell_runtime_spec(binding, service_profile, workload_revision)?;
    validate_current_receipt(command, acknowledgement, "RuntimeRemove")?;
    let NodeCommandPayload::RuntimeRemove { request } = &command.payload else {
        return Err(
            "Durable Cell cleanup requires the existing Fleet RuntimeRemove command".into(),
        );
    };
    if request.unit_id != expected.unit_id || request.generation != expected.generation {
        return Err("Durable Cell RuntimeRemove changed the exact provider generation".into());
    }
    let NodeCommandOutcome::Succeeded { result } = &acknowledgement.outcome else {
        return Err("Durable Cell RuntimeRemove did not succeed".into());
    };
    let NodeCommandResult::RuntimeRemoved { removal } = result.as_ref() else {
        return Err("Durable Cell cleanup receipt is not a RuntimeRemove result".into());
    };
    validate_runtime_evidence_time("removal", removal.removed_at_ms, command, acknowledgement)
}

fn validate_current_receipt(
    command: &NodeCommandEnvelope,
    acknowledgement: &NodeCommandAck,
    operation: &str,
) -> Result<(), String> {
    command.validate()?;
    acknowledgement.validate_against(command)?;
    if acknowledgement.schema != NodeCommandAck::SCHEMA {
        return Err(format!(
            "Durable Cell {operation} admission requires the current Fleet receipt schema"
        ));
    }
    Ok(())
}

fn validate_runtime_evidence_time(
    label: &str,
    observed_at_ms: u64,
    command: &NodeCommandEnvelope,
    acknowledgement: &NodeCommandAck,
) -> Result<(), String> {
    let issued_at_ms = u64::try_from(command.issued_at.timestamp_millis())
        .map_err(|_| format!("Durable Cell {label} command time is invalid"))?;
    let completed_at_ms = u64::try_from(acknowledgement.completed_at.timestamp_millis())
        .map_err(|_| format!("Durable Cell {label} completion time is invalid"))?;
    if observed_at_ms < issued_at_ms || observed_at_ms > completed_at_ms {
        return Err(format!(
            "Durable Cell {label} evidence time falls outside Fleet command execution"
        ));
    }
    Ok(())
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
        } if port == &profile.public_runtime_port
            && path == &profile.health_path
            && expected_statuses.as_slice() == [200] =>
        {
            Ok(())
        }
        _ => Err("Durable Cell Runtime Service changed its public HTTP readiness probe".into()),
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
