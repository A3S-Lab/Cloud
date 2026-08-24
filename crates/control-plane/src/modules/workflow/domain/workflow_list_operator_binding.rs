use super::{
    WorkflowDataSchema, WorkflowDataType, WorkflowListOperatorConfiguration,
    WorkflowRevisionSemanticContracts, WorkflowStepConfiguration, WorkflowStepDescriptorSpec,
    WorkflowStepExecutionClass, WorkflowStepKind, WorkflowStepOwner, WorkflowStepPortCardinality,
    WorkflowStepSpec, WorkflowVariableReadMode,
};
use std::collections::BTreeMap;

const LIST_OPERATOR_DESCRIPTOR_ID: &str = "workflow.list-operator";

pub(crate) fn validate_list_operator_binding(
    step: &WorkflowStepSpec,
    configuration: &WorkflowStepConfiguration,
    input_schema: &WorkflowDataSchema,
    output_schema: &WorkflowDataSchema,
    semantic_contracts: Option<&WorkflowRevisionSemanticContracts>,
) -> Result<(), String> {
    let descriptor = semantic_contracts
        .map(|contracts| contracts.descriptor_spec(&step.id))
        .transpose()?;
    let configured = configuration.list_operator();
    match (configured, descriptor) {
        (None, Some(descriptor)) if claims_list_operator_descriptor(descriptor) => Err(format!(
            "Workflow List Operator step {:?} lost its exact configuration",
            step.id
        )),
        (None, _) => Ok(()),
        (Some(_), None) => Err(format!(
            "Workflow List Operator step {:?} requires immutable descriptor semantic contracts",
            step.id
        )),
        (Some(configuration), Some(descriptor)) => validate_exact_binding(
            step,
            configuration,
            input_schema,
            output_schema,
            descriptor,
            semantic_contracts.ok_or_else(|| {
                "Workflow List Operator semantic authority disappeared".to_owned()
            })?,
        ),
    }
}

fn validate_exact_binding(
    step: &WorkflowStepSpec,
    configuration: &WorkflowListOperatorConfiguration,
    input_schema: &WorkflowDataSchema,
    output_schema: &WorkflowDataSchema,
    descriptor: &WorkflowStepDescriptorSpec,
    semantic_contracts: &WorkflowRevisionSemanticContracts,
) -> Result<(), String> {
    configuration.validate()?;
    if !is_list_operator_descriptor(descriptor)
        || step.kind != WorkflowStepKind::Transform
        || step.capability.is_some()
        || step.policy_digest.is_some()
    {
        return Err(format!(
            "Workflow List Operator step {:?} requires the exact Workflow-owned workflow.list-operator descriptor",
            step.id
        ));
    }
    validate_input_contract(
        &step.id,
        configuration,
        input_schema,
        descriptor,
        semantic_contracts,
    )?;
    validate_output_contract(&step.id, configuration, output_schema, descriptor)
}

fn claims_list_operator_descriptor(descriptor: &WorkflowStepDescriptorSpec) -> bool {
    descriptor.id == LIST_OPERATOR_DESCRIPTOR_ID
        || descriptor.semantic_profile == LIST_OPERATOR_DESCRIPTOR_ID
}

fn is_list_operator_descriptor(descriptor: &WorkflowStepDescriptorSpec) -> bool {
    descriptor.id == LIST_OPERATOR_DESCRIPTOR_ID
        && descriptor.semantic_profile == LIST_OPERATOR_DESCRIPTOR_ID
        && descriptor.owner == WorkflowStepOwner::Workflow
        && descriptor.kind == Some(WorkflowStepKind::Transform)
        && descriptor.execution_class == WorkflowStepExecutionClass::WorkflowLocal
        && descriptor.default_policy_digest.is_none()
        && descriptor.required_bindings.is_empty()
        && descriptor.allowed_capability_types.is_empty()
}

fn validate_input_contract(
    step_id: &str,
    configuration: &WorkflowListOperatorConfiguration,
    input_schema: &WorkflowDataSchema,
    descriptor: &WorkflowStepDescriptorSpec,
    semantic_contracts: &WorkflowRevisionSemanticContracts,
) -> Result<(), String> {
    let input_types = configuration.input_types()?;
    if input_schema.value_type != WorkflowDataType::Object
        || input_schema.fields.len() != input_types.len()
        || descriptor.input_ports.len() != input_types.len()
    {
        return Err(format!(
            "Workflow List Operator step {step_id:?} input schema and descriptor must exactly cover its source and operation inputs"
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
    for (name, value_type) in &input_types {
        let required = *name == configuration.source_port.as_str();
        let field = fields.get(name).ok_or_else(|| {
            format!("Workflow List Operator step {step_id:?} input schema is missing port {name:?}")
        })?;
        let port = ports.get(name).ok_or_else(|| {
            format!("Workflow List Operator step {step_id:?} descriptor is missing port {name:?}")
        })?;
        if field.required != required
            || &field.value_type != value_type
            || port.required != required
            || port.dynamic
            || port.cardinality != WorkflowStepPortCardinality::Single
            || &port.value_type != value_type
        {
            return Err(format!(
                "Workflow List Operator step {step_id:?} input port {name:?} must keep the source required, operation inputs optional, and every port static, single, and type-exact"
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
    if reads.len() != input_types.len() {
        return Err(format!(
            "Workflow List Operator step {step_id:?} variable reads must exactly cover its source and operation inputs"
        ));
    }
    let mut read_ports = BTreeMap::new();
    for read in reads {
        let expected_type = input_types.get(read.target_port.as_str()).ok_or_else(|| {
            format!(
                "Workflow List Operator step {step_id:?} has unrelated variable read {:?}",
                read.id
            )
        })?;
        let required = read.target_port == configuration.source_port;
        if read.required != required
            || read.mode != WorkflowVariableReadMode::DirectValue
            || &read.expected_type != expected_type
            || read_ports
                .insert(read.target_port.as_str(), read.id.as_str())
                .is_some()
        {
            return Err(format!(
                "Workflow List Operator step {step_id:?} read {:?} must keep the source required, operation inputs optional, and every read type-exact and direct",
                read.id
            ));
        }
    }
    Ok(())
}

fn validate_output_contract(
    step_id: &str,
    configuration: &WorkflowListOperatorConfiguration,
    output_schema: &WorkflowDataSchema,
    descriptor: &WorkflowStepDescriptorSpec,
) -> Result<(), String> {
    let output_types = BTreeMap::from([
        ("first_record", (configuration.item_type.clone(), false)),
        ("last_record", (configuration.item_type.clone(), false)),
        ("result", (WorkflowDataType::Array, true)),
    ]);
    if output_schema.value_type != WorkflowDataType::Object
        || output_schema.fields.len() != output_types.len()
        || descriptor.output_ports.len() != output_types.len()
    {
        return Err(format!(
            "Workflow List Operator step {step_id:?} output schema and descriptor must exactly cover result, first_record, and last_record"
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
    for (name, (value_type, required)) in output_types {
        let field = fields.get(name).ok_or_else(|| {
            format!("Workflow List Operator step {step_id:?} output schema is missing {name:?}")
        })?;
        let port = ports.get(name).ok_or_else(|| {
            format!("Workflow List Operator step {step_id:?} descriptor is missing {name:?}")
        })?;
        if field.required != required
            || &field.value_type != &value_type
            || port.required != required
            || port.dynamic
            || port.cardinality != WorkflowStepPortCardinality::Single
            || &port.value_type != &value_type
        {
            return Err(format!(
                "Workflow List Operator step {step_id:?} output {name:?} has an invalid type or cardinality"
            ));
        }
    }
    Ok(())
}
