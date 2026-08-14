use super::super::validation::{
    validate_dotted_identifier, validate_exact_semver, validate_identifier,
};
use super::super::{CapabilityType, WorkflowStepKind};
use super::model::{
    WorkflowStepBindingKind, WorkflowStepDescriptorAdmission, WorkflowStepDescriptorRegistrySpec,
    WorkflowStepDescriptorSpec, WorkflowStepExecutionClass, WorkflowStepFallbackMode,
    WorkflowStepOwner, WorkflowStepPort, WorkflowStepPortCardinality, WorkflowStepPresentationSpec,
    WorkflowStepRetryClassification,
};
use std::collections::BTreeSet;

pub(super) fn normalize_registry_spec(
    mut spec: WorkflowStepDescriptorRegistrySpec,
) -> Result<WorkflowStepDescriptorRegistrySpec, String> {
    validate_dotted_identifier("Workflow descriptor registry ID", &spec.id)?;
    validate_exact_semver("Workflow descriptor registry revision", &spec.revision)?;
    if spec.compiler_schema_version == 0
        || spec.descriptors.is_empty()
        || spec.descriptors.len() > 512
    {
        return Err("Workflow descriptor registry bounds are invalid".into());
    }
    let mut descriptors = spec
        .descriptors
        .into_iter()
        .map(normalize_descriptor_spec)
        .collect::<Result<Vec<_>, _>>()?;
    descriptors.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.revision.cmp(&right.revision))
    });
    if descriptors
        .windows(2)
        .any(|pair| pair[0].id == pair[1].id && pair[0].revision == pair[1].revision)
    {
        return Err("Workflow descriptor registry contains duplicate descriptor revisions".into());
    }
    if descriptors.iter().any(|descriptor| {
        !(descriptor.minimum_compiler_schema_version..=descriptor.maximum_compiler_schema_version)
            .contains(&spec.compiler_schema_version)
    }) {
        return Err(
            "Workflow descriptor registry compiler version is outside a descriptor range".into(),
        );
    }
    spec.descriptors = descriptors;
    Ok(spec)
}

fn normalize_descriptor_spec(
    mut spec: WorkflowStepDescriptorSpec,
) -> Result<WorkflowStepDescriptorSpec, String> {
    validate_dotted_identifier("Workflow descriptor ID", &spec.id)?;
    validate_exact_semver("Workflow descriptor revision", &spec.revision)?;
    validate_dotted_identifier(
        "Workflow descriptor semantic profile",
        &spec.semantic_profile,
    )?;
    if spec.minimum_compiler_schema_version == 0
        || spec.minimum_compiler_schema_version > spec.maximum_compiler_schema_version
    {
        return Err("Workflow descriptor compiler compatibility range is invalid".into());
    }
    validate_ports("input", &mut spec.input_ports)?;
    validate_ports("output", &mut spec.output_ports)?;
    if let Some(error_output) = spec.failure.error_output.as_mut() {
        validate_port("error", error_output)?;
        if spec
            .output_ports
            .iter()
            .any(|port| port.name == error_output.name)
        {
            return Err("Workflow descriptor error and output port names must be distinct".into());
        }
    }
    reject_duplicate_bindings(&spec.required_bindings)?;
    spec.required_bindings.sort();
    reject_duplicate_capability_types(&spec.allowed_capability_types)?;
    spec.allowed_capability_types
        .sort_by(|left, right| left.as_str().cmp(right.as_str()));
    validate_binding_contract(&spec)?;
    validate_execution_class(&spec)?;
    validate_failure_contract(&spec)?;
    validate_admission(&spec)?;
    validate_presentation(&spec.presentation)?;
    Ok(spec)
}

fn validate_ports(label: &str, ports: &mut [WorkflowStepPort]) -> Result<(), String> {
    if ports.len() > 128 {
        return Err(format!("Workflow descriptor has too many {label} ports"));
    }
    for port in ports.iter_mut() {
        validate_port(label, port)?;
    }
    ports.sort_by(|left, right| left.name.cmp(&right.name));
    if ports.windows(2).any(|pair| pair[0].name == pair[1].name) {
        return Err(format!(
            "Workflow descriptor contains duplicate {label} ports"
        ));
    }
    Ok(())
}

fn validate_port(label: &str, port: &WorkflowStepPort) -> Result<(), String> {
    validate_identifier(&format!("Workflow descriptor {label} port"), &port.name)?;
    if port.dynamic && port.cardinality == WorkflowStepPortCardinality::Many {
        return Err("Dynamic Workflow ports cannot also declare many cardinality".into());
    }
    Ok(())
}

fn validate_binding_contract(spec: &WorkflowStepDescriptorSpec) -> Result<(), String> {
    let requires_capability = spec
        .required_bindings
        .contains(&WorkflowStepBindingKind::CapabilityReference);
    if requires_capability == spec.allowed_capability_types.is_empty() {
        return Err(
            "Workflow descriptor capability binding and allowed types must be declared together"
                .into(),
        );
    }
    match spec.kind {
        None if !spec.allowed_capability_types.is_empty() => {
            return Err("Descriptors without a Flow step kind cannot admit capability types".into())
        }
        Some(kind)
            if spec
                .allowed_capability_types
                .iter()
                .any(|capability| !kind.allowed_capability_types().contains(capability)) =>
        {
            return Err(format!(
                "Workflow descriptor capability types do not match coarse kind {:?}",
                kind.as_str()
            ));
        }
        None | Some(_) => {}
    }
    Ok(())
}

fn validate_execution_class(spec: &WorkflowStepDescriptorSpec) -> Result<(), String> {
    match spec.execution_class {
        WorkflowStepExecutionClass::InvocationOnly => {
            if spec.owner != WorkflowStepOwner::Automations || spec.kind.is_some() {
                return Err(
                    "Invocation-only descriptors belong to Automations and are not Flow steps"
                        .into(),
                );
            }
        }
        WorkflowStepExecutionClass::WorkflowLocal => {
            let kind = spec.kind.ok_or_else(|| {
                "Workflow-local descriptors require a coarse Workflow step kind".to_owned()
            })?;
            if spec.owner != WorkflowStepOwner::Workflow || !kind.is_workflow_local() {
                return Err(
                    "Workflow-local descriptors must use a Workflow-owned local step kind".into(),
                );
            }
        }
        WorkflowStepExecutionClass::CompositeRegion => {
            if spec.owner != WorkflowStepOwner::Workflow
                || spec.kind != Some(WorkflowStepKind::Subworkflow)
            {
                return Err(
                    "Composite-region descriptors require the Workflow-owned subworkflow kind"
                        .into(),
                );
            }
        }
        WorkflowStepExecutionClass::OwningApplicationPort => {
            let kind = spec.kind.ok_or_else(|| {
                "Owning-application-port descriptors require a coarse step kind".to_owned()
            })?;
            if !owning_application_port_matches(kind, spec.owner) {
                return Err(
                    "Owning-application-port descriptor kind and owner do not match".into(),
                );
            }
        }
    }
    Ok(())
}

fn owning_application_port_matches(kind: WorkflowStepKind, owner: WorkflowStepOwner) -> bool {
    match kind {
        WorkflowStepKind::Execution => owner == WorkflowStepOwner::Executions,
        WorkflowStepKind::Agent => owner == WorkflowStepOwner::Agents,
        WorkflowStepKind::Mcp => owner == WorkflowStepOwner::Assets,
        WorkflowStepKind::Model => owner == WorkflowStepOwner::Inference,
        WorkflowStepKind::Tool | WorkflowStepKind::Memory => owner == WorkflowStepOwner::Use,
        WorkflowStepKind::Service => matches!(
            owner,
            WorkflowStepOwner::Applications
                | WorkflowStepOwner::Connectors
                | WorkflowStepOwner::Files
                | WorkflowStepOwner::Knowledge
                | WorkflowStepOwner::Sources
        ),
        WorkflowStepKind::Subworkflow => owner == WorkflowStepOwner::Workflow,
        WorkflowStepKind::Output => owner == WorkflowStepOwner::Applications,
        WorkflowStepKind::Input
        | WorkflowStepKind::Transform
        | WorkflowStepKind::Branch
        | WorkflowStepKind::HumanDecision => false,
    }
}

fn validate_failure_contract(spec: &WorkflowStepDescriptorSpec) -> Result<(), String> {
    let failure = &spec.failure;
    if spec.execution_class == WorkflowStepExecutionClass::InvocationOnly
        && (failure.error_output.is_some()
            || failure.retry_classification != WorkflowStepRetryClassification::NotRetryable
            || failure.fallback != WorkflowStepFallbackMode::Unsupported
            || failure.failure_branch)
    {
        return Err("Invocation-only descriptors cannot declare in-run failure behavior".into());
    }
    match failure.fallback {
        WorkflowStepFallbackMode::Unsupported if failure.failure_branch => {
            Err("Unsupported fallback cannot enable a failure branch".into())
        }
        WorkflowStepFallbackMode::FailureBranch
            if !failure.failure_branch || failure.error_output.is_none() =>
        {
            Err("Failure-branch fallback requires a typed error output and branch".into())
        }
        WorkflowStepFallbackMode::FailureBranch => Ok(()),
        WorkflowStepFallbackMode::DefaultOutput
            if failure.failure_branch
                || spec.output_ports.is_empty()
                || spec.default_policy_digest.is_none() =>
        {
            Err("Default fallback requires a typed output and default policy".into())
        }
        WorkflowStepFallbackMode::DefaultOutput | WorkflowStepFallbackMode::Unsupported => Ok(()),
    }
}

fn validate_admission(spec: &WorkflowStepDescriptorSpec) -> Result<(), String> {
    match (spec.admission, spec.unavailable_reason.as_deref()) {
        (WorkflowStepDescriptorAdmission::Admitted, None) => Ok(()),
        (WorkflowStepDescriptorAdmission::Unavailable, Some(reason)) => {
            validate_text("Workflow descriptor unavailable reason", reason, 1, 512)
        }
        _ => Err(
            "Workflow descriptor unavailable reason must exist only for unavailable descriptors"
                .into(),
        ),
    }
}

fn validate_presentation(spec: &WorkflowStepPresentationSpec) -> Result<(), String> {
    validate_text("Workflow descriptor label", &spec.label, 1, 120)?;
    validate_text("Workflow descriptor summary", &spec.summary, 1, 512)?;
    validate_dotted_identifier("Workflow descriptor icon key", &spec.icon_key)
}

fn reject_duplicate_bindings(values: &[WorkflowStepBindingKind]) -> Result<(), String> {
    let unique = values.iter().copied().collect::<BTreeSet<_>>();
    if unique.len() != values.len() {
        return Err("Workflow descriptor contains duplicate required bindings".into());
    }
    Ok(())
}

fn reject_duplicate_capability_types(values: &[CapabilityType]) -> Result<(), String> {
    let unique = values
        .iter()
        .map(|value| value.as_str())
        .collect::<BTreeSet<_>>();
    if unique.len() != values.len() {
        return Err("Workflow descriptor contains duplicate capability types".into());
    }
    Ok(())
}

fn validate_text(label: &str, value: &str, minimum: usize, maximum: usize) -> Result<(), String> {
    let length = value.chars().count();
    if !(minimum..=maximum).contains(&length)
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(format!(
            "{label} must contain {minimum}-{maximum} trimmed printable characters"
        ));
    }
    Ok(())
}
