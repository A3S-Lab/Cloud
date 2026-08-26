use super::super::{
    CapabilityReference, CapabilityType, WorkflowCompositeRegions, WorkflowDataType,
    WorkflowStepBindingKind, WorkflowStepDescriptorBindings, WorkflowStepDescriptorSpec,
    WorkflowStepExecutionClass, WorkflowStepFailureContract, WorkflowStepFallbackMode,
    WorkflowStepKind, WorkflowStepOwner, WorkflowStepPortCardinality,
    WorkflowStepRetryClassification, WorkflowStepSpec, WorkflowVariableContract,
    WorkflowVariableContractSpec, WorkflowVariableDefaults,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(super) fn descriptor_has_runtime_dispatch(descriptor: &WorkflowStepDescriptorSpec) -> bool {
    match descriptor.execution_class {
        WorkflowStepExecutionClass::WorkflowLocal | WorkflowStepExecutionClass::CompositeRegion => {
            true
        }
        WorkflowStepExecutionClass::OwningApplicationPort => {
            match (descriptor.owner, descriptor.kind) {
                (WorkflowStepOwner::Executions, Some(WorkflowStepKind::Execution)) => {
                    is_exact_finite_execution_descriptor(descriptor)
                }
                (WorkflowStepOwner::Agents, Some(WorkflowStepKind::Agent)) => {
                    is_exact_agent_descriptor(descriptor)
                }
                (WorkflowStepOwner::Connectors, Some(WorkflowStepKind::Service)) => true,
                (WorkflowStepOwner::Applications, Some(WorkflowStepKind::Service)) => {
                    is_exact_application_variable_descriptor(descriptor)
                }
                (WorkflowStepOwner::Applications, Some(WorkflowStepKind::Output)) => {
                    is_exact_application_answer_descriptor(descriptor)
                }
                _ => false,
            }
        }
        WorkflowStepExecutionClass::InvocationOnly => false,
    }
}

fn is_exact_finite_execution_descriptor(descriptor: &WorkflowStepDescriptorSpec) -> bool {
    descriptor.id == "executions.finite"
        && descriptor.semantic_profile == "executions.finite"
        && descriptor.owner == WorkflowStepOwner::Executions
        && descriptor.kind == Some(WorkflowStepKind::Execution)
        && descriptor.execution_class == WorkflowStepExecutionClass::OwningApplicationPort
}

fn is_exact_agent_descriptor(descriptor: &WorkflowStepDescriptorSpec) -> bool {
    let exact_failure = (descriptor.failure.error_output.is_none()
        && descriptor.failure.fallback == WorkflowStepFallbackMode::Unsupported
        && !descriptor.failure.failure_branch)
        || descriptor
            .failure
            .error_output
            .as_ref()
            .is_some_and(|output| {
                output.name == "error"
                    && output.value_type == WorkflowDataType::Object
                    && output.cardinality == WorkflowStepPortCardinality::Single
                    && output.required
                    && !output.dynamic
                    && descriptor.failure.fallback == WorkflowStepFallbackMode::FailureBranch
                    && descriptor.failure.failure_branch
            });
    matches!(descriptor.id.as_str(), "agent.classic" | "agent.release")
        && descriptor.semantic_profile == descriptor.id
        && descriptor.owner == WorkflowStepOwner::Agents
        && descriptor.kind == Some(WorkflowStepKind::Agent)
        && descriptor.execution_class == WorkflowStepExecutionClass::OwningApplicationPort
        && descriptor.required_bindings == [WorkflowStepBindingKind::CapabilityReference]
        && descriptor.allowed_capability_types == [CapabilityType::AgentRelease]
        && descriptor.default_policy_digest.is_none()
        && descriptor.failure.retry_classification
            == WorkflowStepRetryClassification::OwnerClassified
        && exact_failure
}

pub(super) fn is_exact_agent_release_capability(capability: Option<&CapabilityReference>) -> bool {
    capability.is_some_and(|capability| {
        capability.capability_type == CapabilityType::AgentRelease
            && capability.capability == "agent.execute"
            && uuid::Uuid::parse_str(&capability.revision).is_ok_and(|revision| !revision.is_nil())
    })
}

pub(super) fn validate_connector_retry_authority(
    step: &WorkflowStepSpec,
    descriptor: &WorkflowStepDescriptorSpec,
) -> Result<(), String> {
    let connector = step
        .capability
        .as_ref()
        .is_some_and(|capability| capability.capability_type == CapabilityType::ConnectorRevision);
    if !connector {
        return Ok(());
    }
    if descriptor.owner != WorkflowStepOwner::Connectors
        || descriptor.semantic_profile != "connector.http"
        || descriptor.failure.retry_classification
            != WorkflowStepRetryClassification::OwnerClassified
    {
        return Err(format!(
            "Workflow Connector step {:?} lacks Connectors-owned retry classification",
            step.id
        ));
    }
    Ok(())
}

pub(super) fn validate_default_output_authority(
    step: &WorkflowStepSpec,
    descriptor: &WorkflowStepDescriptorSpec,
) -> Result<(), String> {
    if descriptor.failure.fallback != WorkflowStepFallbackMode::DefaultOutput {
        return Ok(());
    }
    if step.kind != WorkflowStepKind::Execution
        || descriptor.owner != WorkflowStepOwner::Executions
        || descriptor.execution_class != WorkflowStepExecutionClass::OwningApplicationPort
        || step.policy_digest.as_ref() != descriptor.default_policy_digest.as_ref()
        || descriptor.failure.error_output.is_some()
        || descriptor.failure.retry_classification
            != WorkflowStepRetryClassification::OwnerClassified
        || !step.capability.as_ref().is_some_and(|capability| {
            capability.capability_type == CapabilityType::ExecutionTemplate
        })
    {
        return Err(format!(
            "Workflow step {:?} default-output fallback requires the Executions-owned finite Execution port and its exact descriptor policy",
            step.id
        ));
    }
    let [port] = descriptor.output_ports.as_slice() else {
        return Err(format!(
            "Workflow default-output step {:?} must expose exactly one output port",
            step.id
        ));
    };
    if port.cardinality != WorkflowStepPortCardinality::Single || !port.required || port.dynamic {
        return Err(format!(
            "Workflow default-output step {:?} must expose one required static output port",
            step.id
        ));
    }
    Ok(())
}

pub(super) fn validate_variable_read_ports(
    variables: &WorkflowVariableContractSpec,
    descriptors: &BTreeMap<&str, &WorkflowStepDescriptorSpec>,
) -> Result<(), String> {
    for read in &variables.reads {
        let descriptor = descriptors
            .get(read.consumer_step_id.as_str())
            .ok_or_else(|| {
                format!(
                    "Workflow variable read {:?} has no consumer descriptor",
                    read.id
                )
            })?;
        let port = descriptor
            .input_ports
            .iter()
            .find(|port| port.name == read.target_port)
            .ok_or_else(|| {
                format!(
                    "Workflow variable read {:?} targets undeclared descriptor input {:?}",
                    read.id, read.target_port
                )
            })?;
        if port.value_type != WorkflowDataType::Any && port.value_type != read.expected_type {
            return Err(format!(
                "Workflow variable read {:?} type does not match descriptor input {:?}",
                read.id, read.target_port
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_supported_bindings(
    step: &WorkflowStepSpec,
    descriptor: &WorkflowStepDescriptorSpec,
) -> Result<(), String> {
    if let Some(unsupported) = descriptor.required_bindings.iter().find(|binding| {
        !matches!(
            binding,
            WorkflowStepBindingKind::CapabilityReference | WorkflowStepBindingKind::PlacementPolicy
        ) && !(*binding == &WorkflowStepBindingKind::ReleaseReference
            && (is_exact_application_answer_descriptor(descriptor)
                || is_exact_application_variable_descriptor(descriptor)))
    }) {
        return Err(format!(
            "Workflow step {:?} descriptor requires unsupported {} binding",
            step.id,
            unsupported.as_str()
        ));
    }
    Ok(())
}

pub(super) fn is_exact_application_answer_descriptor(
    descriptor: &WorkflowStepDescriptorSpec,
) -> bool {
    descriptor.id == "application.answer"
        && descriptor.semantic_profile == "application.answer"
        && descriptor.owner == WorkflowStepOwner::Applications
        && descriptor.kind == Some(WorkflowStepKind::Output)
        && descriptor.execution_class == WorkflowStepExecutionClass::OwningApplicationPort
        && descriptor.required_bindings == [WorkflowStepBindingKind::ReleaseReference]
        && descriptor.allowed_capability_types.is_empty()
        && descriptor.default_policy_digest.is_none()
        && is_application_answer_failure_contract(&descriptor.failure)
}

fn is_application_answer_failure_contract(failure: &WorkflowStepFailureContract) -> bool {
    (failure.error_output.is_none()
        && failure.retry_classification == WorkflowStepRetryClassification::NotRetryable
        && failure.fallback == WorkflowStepFallbackMode::Unsupported
        && !failure.failure_branch)
        || (failure.error_output.as_ref().is_some_and(|output| {
            output.name == "error"
                && output.value_type == WorkflowDataType::Object
                && output.cardinality == WorkflowStepPortCardinality::Single
                && output.required
                && !output.dynamic
        }) && failure.retry_classification == WorkflowStepRetryClassification::OwnerClassified
            && failure.fallback == WorkflowStepFallbackMode::FailureBranch
            && failure.failure_branch)
}

pub(super) fn is_exact_workflow_output_descriptor(descriptor: &WorkflowStepDescriptorSpec) -> bool {
    descriptor.id == "workflow.output"
        && descriptor.semantic_profile == "workflow.output"
        && descriptor.owner == WorkflowStepOwner::Workflow
        && descriptor.kind == Some(WorkflowStepKind::Output)
        && descriptor.execution_class == WorkflowStepExecutionClass::WorkflowLocal
}

pub(super) fn is_exact_application_variable_descriptor(
    descriptor: &WorkflowStepDescriptorSpec,
) -> bool {
    descriptor.id == "application.conversation-variable-assign"
        && descriptor.semantic_profile == "application.conversation-variable-assign"
        && descriptor.owner == WorkflowStepOwner::Applications
        && descriptor.kind == Some(WorkflowStepKind::Service)
        && descriptor.execution_class == WorkflowStepExecutionClass::OwningApplicationPort
        && descriptor.required_bindings == [WorkflowStepBindingKind::ReleaseReference]
        && descriptor.allowed_capability_types.is_empty()
        && descriptor.default_policy_digest.is_none()
        && is_application_variable_failure_contract(&descriptor.failure)
}

fn is_application_variable_failure_contract(failure: &WorkflowStepFailureContract) -> bool {
    (failure.error_output.is_none()
        && failure.retry_classification == WorkflowStepRetryClassification::NotRetryable
        && failure.fallback == WorkflowStepFallbackMode::Unsupported
        && !failure.failure_branch)
        || (failure.error_output.as_ref().is_some_and(|output| {
            output.name == "error"
                && output.value_type == WorkflowDataType::Object
                && output.cardinality == WorkflowStepPortCardinality::Single
                && output.required
                && !output.dynamic
        }) && failure.retry_classification == WorkflowStepRetryClassification::OwnerClassified
            && failure.fallback == WorkflowStepFallbackMode::FailureBranch
            && failure.failure_branch)
}

pub(super) fn is_exact_application_final_output_descriptor(
    descriptor: &WorkflowStepDescriptorSpec,
) -> bool {
    descriptor.id == "workflow.output"
        && descriptor.semantic_profile == "workflow.output"
        && descriptor.owner == WorkflowStepOwner::Workflow
        && descriptor.kind == Some(WorkflowStepKind::Output)
        && descriptor.execution_class == WorkflowStepExecutionClass::WorkflowLocal
        && descriptor
            .required_bindings
            .iter()
            .all(|binding| *binding == WorkflowStepBindingKind::PlacementPolicy)
        && descriptor.allowed_capability_types.is_empty()
        && descriptor.default_policy_digest.is_none()
        && descriptor.failure.error_output.is_none()
        && descriptor.failure.retry_classification == WorkflowStepRetryClassification::NotRetryable
        && descriptor.failure.fallback == WorkflowStepFallbackMode::Unsupported
        && !descriptor.failure.failure_branch
}

pub(super) fn validate_capability_binding(
    step: &WorkflowStepSpec,
    descriptor: &WorkflowStepDescriptorSpec,
) -> Result<(), String> {
    if step.kind == WorkflowStepKind::Service
        && step.capability.is_none()
        && !is_exact_application_variable_descriptor(descriptor)
    {
        return Err(format!(
            "Workflow capability-free Service step {:?} is not the exact Application variable port",
            step.id
        ));
    }
    let requires_capability = descriptor
        .required_bindings
        .contains(&WorkflowStepBindingKind::CapabilityReference);
    match (requires_capability, step.capability.as_ref()) {
        (true, Some(capability))
            if descriptor
                .allowed_capability_types
                .contains(&capability.capability_type) =>
        {
            Ok(())
        }
        (false, None) => Ok(()),
        _ => Err(format!(
            "Workflow step {:?} capability does not satisfy its descriptor",
            step.id
        )),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SemanticContractDigestInput<'a> {
    descriptor_bindings_digest: &'a str,
    variable_contract_digest: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    variable_defaults_digest: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    composite_regions_digest: Option<&'a str>,
}

pub(super) fn digest_contract_set(
    bindings: &WorkflowStepDescriptorBindings,
    variables: &WorkflowVariableContract,
    defaults: Option<&WorkflowVariableDefaults>,
    composite_regions: Option<&WorkflowCompositeRegions>,
) -> Result<Sha256Digest, String> {
    let encoded = serde_json::to_vec(&SemanticContractDigestInput {
        descriptor_bindings_digest: bindings.digest().as_str(),
        variable_contract_digest: variables.digest().as_str(),
        variable_defaults_digest: defaults.map(|value| value.digest().as_str()),
        composite_regions_digest: composite_regions.map(|value| value.digest().as_str()),
    })
    .map_err(|error| format!("could not encode Workflow semantic contract set: {error}"))?;
    Sha256Digest::parse(format!("sha256:{:x}", Sha256::digest(encoded)))
}

pub(super) fn validate_default_material(
    variables: &WorkflowVariableContract,
    defaults: Option<&WorkflowVariableDefaults>,
) -> Result<(), String> {
    let requires_defaults = variables
        .spec()
        .declarations
        .iter()
        .any(|declaration| declaration.default_value_digest.is_some());
    match (requires_defaults, defaults) {
        (false, None) => Ok(()),
        (true, Some(defaults)) => defaults.validate_contract(variables),
        (true, None) => Err(
            "Workflow variable contract declares digest-only defaults without immutable material"
                .into(),
        ),
        (false, Some(_)) => {
            Err("Workflow variable defaults are present without digest-backed declarations".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::workflow::domain::{
        WorkflowStepDescriptorAdmission, WorkflowStepPort, WorkflowStepPresentationSpec,
    };

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    fn port(name: &str, value_type: WorkflowDataType) -> WorkflowStepPort {
        WorkflowStepPort {
            name: name.into(),
            value_type,
            cardinality: WorkflowStepPortCardinality::Single,
            required: true,
            dynamic: false,
        }
    }

    fn exact_agent_descriptor() -> WorkflowStepDescriptorSpec {
        WorkflowStepDescriptorSpec {
            id: "agent.release".into(),
            revision: "1.0.0".into(),
            owner: WorkflowStepOwner::Agents,
            kind: Some(WorkflowStepKind::Agent),
            semantic_profile: "agent.release".into(),
            execution_class: WorkflowStepExecutionClass::OwningApplicationPort,
            input_ports: vec![port("request", WorkflowDataType::Object)],
            output_ports: vec![port("result", WorkflowDataType::Object)],
            configuration_schema_digest: digest('a'),
            default_policy_digest: None,
            required_bindings: vec![WorkflowStepBindingKind::CapabilityReference],
            allowed_capability_types: vec![CapabilityType::AgentRelease],
            failure: WorkflowStepFailureContract {
                error_output: None,
                retry_classification: WorkflowStepRetryClassification::OwnerClassified,
                fallback: WorkflowStepFallbackMode::Unsupported,
                failure_branch: false,
            },
            minimum_compiler_schema_version: 2,
            maximum_compiler_schema_version: 2,
            admission: WorkflowStepDescriptorAdmission::Admitted,
            unavailable_reason: None,
            presentation: WorkflowStepPresentationSpec {
                label: "Agent".into(),
                summary: "Executes one exact Agent release".into(),
                icon_key: "agent.release".into(),
            },
        }
    }

    #[test]
    fn exact_agent_dispatch_admits_only_legacy_unsupported_or_typed_object_failure_branch() {
        let legacy = exact_agent_descriptor();
        assert!(descriptor_has_runtime_dispatch(&legacy));

        let mut routed = legacy.clone();
        routed.failure.error_output = Some(port("error", WorkflowDataType::Object));
        routed.failure.fallback = WorkflowStepFallbackMode::FailureBranch;
        routed.failure.failure_branch = true;
        assert!(descriptor_has_runtime_dispatch(&routed));

        let mut wrong_name = routed.clone();
        wrong_name
            .failure
            .error_output
            .as_mut()
            .expect("error port")
            .name = "failed".into();
        assert!(!descriptor_has_runtime_dispatch(&wrong_name));

        let mut wrong_type = routed.clone();
        wrong_type
            .failure
            .error_output
            .as_mut()
            .expect("error port")
            .value_type = WorkflowDataType::String;
        assert!(!descriptor_has_runtime_dispatch(&wrong_type));

        let mut wrong_fallback = routed;
        wrong_fallback.failure.fallback = WorkflowStepFallbackMode::Unsupported;
        assert!(!descriptor_has_runtime_dispatch(&wrong_fallback));
    }
}
