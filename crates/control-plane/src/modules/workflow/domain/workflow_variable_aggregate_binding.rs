use super::{
    WorkflowDataSchema, WorkflowDataType, WorkflowRevisionSemanticContracts,
    WorkflowStepConfiguration, WorkflowStepDescriptorSpec, WorkflowStepExecutionClass,
    WorkflowStepKind, WorkflowStepOwner, WorkflowStepPortCardinality, WorkflowStepSpec,
    WorkflowVariableReadMode,
};
use std::collections::{BTreeMap, BTreeSet};

const VARIABLE_AGGREGATE_DESCRIPTOR_ID: &str = "workflow.variable-aggregate";

pub(crate) fn validate_variable_aggregate_binding(
    step: &WorkflowStepSpec,
    configuration: &WorkflowStepConfiguration,
    input_schema: &WorkflowDataSchema,
    output_schema: &WorkflowDataSchema,
    semantic_contracts: Option<&WorkflowRevisionSemanticContracts>,
) -> Result<(), String> {
    let descriptor = semantic_contracts
        .map(|contracts| contracts.descriptor_spec(&step.id))
        .transpose()?;
    let configured = configuration.variable_aggregate();
    match (configured, descriptor) {
        (None, Some(descriptor)) if claims_variable_aggregate_descriptor(descriptor) => Err(format!(
            "Workflow Variable Aggregator step {:?} lost its exact configuration",
            step.id
        )),
        (None, _) => Ok(()),
        (Some(_), None) => Err(format!(
            "Workflow Variable Aggregator step {:?} requires immutable descriptor semantic contracts",
            step.id
        )),
        (Some(configuration), Some(descriptor)) => validate_exact_binding(
            step,
            configuration,
            input_schema,
            output_schema,
            descriptor,
            semantic_contracts.ok_or_else(|| {
                "Workflow Variable Aggregator semantic authority disappeared".to_owned()
            })?,
        ),
    }
}

fn validate_exact_binding(
    step: &WorkflowStepSpec,
    configuration: &super::WorkflowVariableAggregateConfiguration,
    input_schema: &WorkflowDataSchema,
    output_schema: &WorkflowDataSchema,
    descriptor: &WorkflowStepDescriptorSpec,
    semantic_contracts: &WorkflowRevisionSemanticContracts,
) -> Result<(), String> {
    configuration.validate()?;
    if !is_variable_aggregate_descriptor(descriptor)
        || step.kind != WorkflowStepKind::Transform
        || step.capability.is_some()
        || step.policy_digest.is_some()
    {
        return Err(format!(
            "Workflow Variable Aggregator step {:?} requires the exact Workflow-owned workflow.variable-aggregate descriptor",
            step.id
        ));
    }

    let candidate_types = configuration.candidate_types()?;
    validate_input_contract(
        &step.id,
        &candidate_types,
        input_schema,
        descriptor,
        semantic_contracts,
    )?;
    validate_output_contract(&step.id, configuration, output_schema, descriptor)
}

fn claims_variable_aggregate_descriptor(descriptor: &WorkflowStepDescriptorSpec) -> bool {
    descriptor.id == VARIABLE_AGGREGATE_DESCRIPTOR_ID
        || descriptor.semantic_profile == VARIABLE_AGGREGATE_DESCRIPTOR_ID
}

fn is_variable_aggregate_descriptor(descriptor: &WorkflowStepDescriptorSpec) -> bool {
    descriptor.id == VARIABLE_AGGREGATE_DESCRIPTOR_ID
        && descriptor.semantic_profile == VARIABLE_AGGREGATE_DESCRIPTOR_ID
        && descriptor.owner == WorkflowStepOwner::Workflow
        && descriptor.kind == Some(WorkflowStepKind::Transform)
        && descriptor.execution_class == WorkflowStepExecutionClass::WorkflowLocal
        && descriptor.default_policy_digest.is_none()
        && descriptor.required_bindings.is_empty()
        && descriptor.allowed_capability_types.is_empty()
}

fn validate_input_contract(
    step_id: &str,
    candidate_types: &BTreeMap<&str, &WorkflowDataType>,
    input_schema: &WorkflowDataSchema,
    descriptor: &WorkflowStepDescriptorSpec,
    semantic_contracts: &WorkflowRevisionSemanticContracts,
) -> Result<(), String> {
    if input_schema.value_type != WorkflowDataType::Object
        || input_schema.fields.len() != candidate_types.len()
        || descriptor.input_ports.len() != candidate_types.len()
    {
        return Err(format!(
            "Workflow Variable Aggregator step {step_id:?} input schema and descriptor must exactly cover its candidate ports"
        ));
    }
    let fields = input_schema
        .fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect::<BTreeMap<_, _>>();
    let ports = descriptor
        .input_ports
        .iter()
        .map(|port| (port.name.as_str(), port))
        .collect::<BTreeMap<_, _>>();
    for (name, value_type) in candidate_types {
        let field = fields.get(name).ok_or_else(|| {
            format!(
                "Workflow Variable Aggregator step {step_id:?} input schema is missing candidate port {name:?}"
            )
        })?;
        let port = ports.get(name).ok_or_else(|| {
            format!(
                "Workflow Variable Aggregator step {step_id:?} descriptor is missing candidate port {name:?}"
            )
        })?;
        if field.required
            || &field.value_type != *value_type
            || port.required
            || port.dynamic
            || port.cardinality != WorkflowStepPortCardinality::Single
            || &port.value_type != *value_type
        {
            return Err(format!(
                "Workflow Variable Aggregator step {step_id:?} candidate port {name:?} must be optional, static, single, and type-exact"
            ));
        }
    }

    let reads = semantic_contracts
        .variable_contract()
        .spec()
        .reads
        .iter()
        .filter(|read| read.consumer_step_id == step_id)
        .collect::<Vec<_>>();
    if reads.len() != candidate_types.len() {
        return Err(format!(
            "Workflow Variable Aggregator step {step_id:?} variable reads must exactly cover its candidate ports"
        ));
    }
    let mut read_ports = BTreeSet::new();
    for read in reads {
        let expected_type = candidate_types
            .get(read.target_port.as_str())
            .ok_or_else(|| {
                format!(
                "Workflow Variable Aggregator step {step_id:?} has unrelated variable read {:?}",
                read.id
            )
            })?;
        if read.required
            || read.mode != WorkflowVariableReadMode::DirectValue
            || &read.expected_type != *expected_type
            || !read_ports.insert(read.target_port.as_str())
        {
            return Err(format!(
                "Workflow Variable Aggregator step {step_id:?} read {:?} must be one optional type-exact direct candidate",
                read.id
            ));
        }
    }
    Ok(())
}

fn validate_output_contract(
    step_id: &str,
    configuration: &super::WorkflowVariableAggregateConfiguration,
    output_schema: &WorkflowDataSchema,
    descriptor: &WorkflowStepDescriptorSpec,
) -> Result<(), String> {
    let output_types = if configuration.group_enabled {
        configuration
            .groups
            .iter()
            .map(|group| (group.output_port.as_str(), WorkflowDataType::Object))
            .collect::<BTreeMap<_, _>>()
    } else {
        let group = configuration.groups.first().ok_or_else(|| {
            format!("Workflow Variable Aggregator step {step_id:?} requires one output group")
        })?;
        BTreeMap::from([("output", group.output_type.clone())])
    };
    if output_schema.value_type != WorkflowDataType::Object
        || output_schema.fields.len() != output_types.len()
        || descriptor.output_ports.len() != output_types.len()
    {
        return Err(format!(
            "Workflow Variable Aggregator step {step_id:?} output schema and descriptor must exactly cover its configured outputs"
        ));
    }
    let fields = output_schema
        .fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect::<BTreeMap<_, _>>();
    let ports = descriptor
        .output_ports
        .iter()
        .map(|port| (port.name.as_str(), port))
        .collect::<BTreeMap<_, _>>();
    for (name, value_type) in output_types {
        let field = fields.get(name).ok_or_else(|| {
            format!(
                "Workflow Variable Aggregator step {step_id:?} output schema is missing {name:?}"
            )
        })?;
        let port = ports.get(name).ok_or_else(|| {
            format!(
                "Workflow Variable Aggregator step {step_id:?} descriptor is missing output {name:?}"
            )
        })?;
        if !field.required
            || field.value_type != value_type
            || !port.required
            || port.dynamic
            || port.cardinality != WorkflowStepPortCardinality::Single
            || port.value_type != value_type
        {
            return Err(format!(
                "Workflow Variable Aggregator step {step_id:?} output {name:?} must be required, static, single, and type-exact"
            ));
        }
    }
    Ok(())
}
