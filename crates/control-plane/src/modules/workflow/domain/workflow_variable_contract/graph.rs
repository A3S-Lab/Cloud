use super::super::{WorkflowSpec, WorkflowStepKind};
use super::model::{
    WorkflowVariableAssignment, WorkflowVariableContractSpec, WorkflowVariableDeclaration,
    WorkflowVariableScope,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn validate_graph_bindings(
    contract: &WorkflowVariableContractSpec,
    workflow: &WorkflowSpec,
    application_ports: &BTreeSet<&str>,
) -> Result<(), String> {
    let order = workflow.topological_order(Default::default())?;
    let steps = workflow
        .steps
        .iter()
        .map(|step| (step.id.as_str(), step))
        .collect::<BTreeMap<_, _>>();
    let input = workflow
        .steps
        .iter()
        .find(|step| step.kind == WorkflowStepKind::Input)
        .ok_or_else(|| "Workflow variable graph has no input step".to_owned())?;
    let predecessors = predecessors(workflow, &steps)?;
    let dominators = dominators(&order, input.id.as_str(), &predecessors)?;
    let reachable = transitive_reachability(&order, workflow, &steps)?;
    let declarations = contract
        .declarations
        .iter()
        .map(|value| (value.name.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    let assignments = assignments_by_target(&contract.assignments);

    for declaration in &contract.declarations {
        validate_declaration_binding(declaration, input, &steps)?;
    }
    for read in &contract.reads {
        if !steps.contains_key(read.consumer_step_id.as_str()) {
            return Err(format!(
                "Workflow variable read {:?} references a missing consumer step",
                read.id
            ));
        }
        validate_region_binding(
            read.consumer_region_id.as_deref(),
            read.consumer_step_id.as_str(),
            &steps,
        )?;
        let declaration = declarations
            .get(read.variable.as_str())
            .ok_or_else(|| format!("Workflow variable {:?} disappeared", read.variable))?;
        validate_variable_available(
            declaration,
            read.consumer_step_id.as_str(),
            read.required,
            assignments.get(read.variable.as_str()).map(Vec::as_slice),
            &dominators,
            &reachable,
            application_ports,
        )?;
    }

    let mut previous_writer_by_target = BTreeMap::<&str, &str>::new();
    for assignment in &contract.assignments {
        if !steps.contains_key(assignment.writer_step_id.as_str()) {
            return Err(format!(
                "Workflow variable assignment {:?} references a missing writer step",
                assignment.id
            ));
        }
        validate_region_binding(
            assignment.writer_region_id.as_deref(),
            assignment.writer_step_id.as_str(),
            &steps,
        )?;
        let source = declarations
            .get(assignment.source_variable.as_str())
            .ok_or_else(|| {
                format!(
                    "assignment source {:?} disappeared",
                    assignment.source_variable
                )
            })?;
        let target = declarations
            .get(assignment.target_variable.as_str())
            .ok_or_else(|| {
                format!(
                    "assignment target {:?} disappeared",
                    assignment.target_variable
                )
            })?;
        if target.scope == WorkflowVariableScope::Application
            && !application_ports.contains(assignment.writer_step_id.as_str())
        {
            return Err(
                "Application variable writes require a descriptor-bound Applications port".into(),
            );
        }
        validate_assignment_source(
            assignment,
            source,
            assignments
                .get(assignment.source_variable.as_str())
                .map(Vec::as_slice),
            &dominators,
            &reachable,
            application_ports,
        )?;
        for evidence in [
            assignment.expected_revision_variable.as_deref(),
            assignment.idempotency_key_variable.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            let declaration = declarations
                .get(evidence)
                .ok_or_else(|| format!("Workflow assignment evidence {evidence:?} disappeared"))?;
            validate_variable_available(
                declaration,
                assignment.writer_step_id.as_str(),
                true,
                assignments.get(evidence).map(Vec::as_slice),
                &dominators,
                &reachable,
                application_ports,
            )?;
        }
        if let Some(previous) = previous_writer_by_target.insert(
            assignment.target_variable.as_str(),
            assignment.writer_step_id.as_str(),
        ) {
            if !strictly_precedes(previous, assignment.writer_step_id.as_str(), &reachable) {
                return Err(format!(
                    "Workflow variable assignments for {:?} are not in deterministic graph order",
                    assignment.target_variable
                ));
            }
        }
    }

    for export in &contract.exports {
        let region = steps.get(export.region_id.as_str()).ok_or_else(|| {
            format!(
                "Workflow variable export {:?} references a missing region",
                export.id
            )
        })?;
        if region.kind != WorkflowStepKind::Subworkflow {
            return Err("Workflow variable export region is not a composite step".into());
        }
    }
    Ok(())
}

fn validate_declaration_binding(
    value: &WorkflowVariableDeclaration,
    input: &super::super::WorkflowStepSpec,
    steps: &BTreeMap<&str, &super::super::WorkflowStepSpec>,
) -> Result<(), String> {
    match value.scope {
        WorkflowVariableScope::InvocationInput if value.source_path.is_empty() => {
            if value.source_schema_digest.as_ref() != Some(&input.output_schema_digest) {
                return Err(
                    "Whole invocation-input variable must bind the input step output schema".into(),
                );
            }
        }
        WorkflowVariableScope::InvocationInput => {
            if value.source_schema_digest.as_ref() != Some(&input.output_schema_digest) {
                return Err("Invocation-input variable root schema does not match Input".into());
            }
        }
        WorkflowVariableScope::NodeOutput => {
            let source_id = value
                .source_step_id
                .as_deref()
                .ok_or_else(|| "node-output variable lost its source step".to_owned())?;
            let source = steps.get(source_id).ok_or_else(|| {
                format!(
                    "node-output variable {:?} references a missing step",
                    value.name
                )
            })?;
            if value.source_schema_digest.as_ref() != Some(&source.output_schema_digest) {
                return Err(
                    "Node-output variable root schema does not match its source step".into(),
                );
            }
        }
        WorkflowVariableScope::CompositeLocal => {
            let region_id = value
                .region_id
                .as_deref()
                .ok_or_else(|| "composite-local variable lost its region".to_owned())?;
            let region = steps.get(region_id).ok_or_else(|| {
                format!(
                    "composite-local variable {:?} references a missing region",
                    value.name
                )
            })?;
            if region.kind != WorkflowStepKind::Subworkflow {
                return Err("Composite-local variable region is not a composite step".into());
            }
            if value.mutation_mode == super::model::WorkflowVariableMutationMode::Immutable
                && value.source_schema_digest.as_ref() != Some(&region.input_schema_digest)
            {
                return Err(
                    "Composite input variable root schema does not match its region".into(),
                );
            }
        }
        WorkflowVariableScope::Run | WorkflowVariableScope::Application => {}
    }
    Ok(())
}

fn validate_variable_available(
    declaration: &WorkflowVariableDeclaration,
    consumer: &str,
    required: bool,
    assignments: Option<&[&WorkflowVariableAssignment]>,
    dominators: &BTreeMap<String, BTreeSet<String>>,
    reachable: &BTreeMap<String, BTreeSet<String>>,
    application_ports: &BTreeSet<&str>,
) -> Result<(), String> {
    match declaration.scope {
        WorkflowVariableScope::InvocationInput => Ok(()),
        WorkflowVariableScope::Application if application_ports.contains(consumer) => Ok(()),
        WorkflowVariableScope::Application => {
            Err("Application variable reads require a descriptor-bound Applications port".into())
        }
        WorkflowVariableScope::NodeOutput => {
            let source = declaration
                .source_step_id
                .as_deref()
                .ok_or_else(|| "node-output variable lost its source step".to_owned())?;
            require_predecessor(source, consumer, required, dominators, reachable)
        }
        WorkflowVariableScope::CompositeLocal
            if declaration.mutation_mode
                == super::model::WorkflowVariableMutationMode::Immutable =>
        {
            Ok(())
        }
        WorkflowVariableScope::Run | WorkflowVariableScope::CompositeLocal => {
            let preceding = preceding_assignments(assignments, consumer, reachable);
            if preceding.is_empty() {
                return if declaration.default_value_digest.is_some() {
                    Ok(())
                } else {
                    Err(format!(
                        "Workflow variable {:?} is read before an assignment",
                        declaration.name
                    ))
                };
            }
            if required
                && declaration.default_value_digest.is_none()
                && !preceding.iter().any(|assignment| {
                    dominators
                        .get(consumer)
                        .is_some_and(|values| values.contains(&assignment.writer_step_id))
                })
            {
                return Err(format!(
                    "Required Workflow variable {:?} has no dominating assignment",
                    declaration.name
                ));
            }
            Ok(())
        }
    }
}

fn validate_assignment_source(
    assignment: &WorkflowVariableAssignment,
    source: &WorkflowVariableDeclaration,
    source_assignments: Option<&[&WorkflowVariableAssignment]>,
    dominators: &BTreeMap<String, BTreeSet<String>>,
    reachable: &BTreeMap<String, BTreeSet<String>>,
    application_ports: &BTreeSet<&str>,
) -> Result<(), String> {
    match source.scope {
        WorkflowVariableScope::InvocationInput => Ok(()),
        WorkflowVariableScope::Application
            if application_ports.contains(assignment.writer_step_id.as_str()) =>
        {
            Ok(())
        }
        WorkflowVariableScope::Application => {
            Err("Application variable reads require a descriptor-bound Applications port".into())
        }
        WorkflowVariableScope::NodeOutput => {
            let source_step = source
                .source_step_id
                .as_deref()
                .ok_or_else(|| "node-output assignment source lost its step".to_owned())?;
            if source_step == assignment.writer_step_id {
                Ok(())
            } else {
                require_predecessor(
                    source_step,
                    &assignment.writer_step_id,
                    true,
                    dominators,
                    reachable,
                )
            }
        }
        WorkflowVariableScope::CompositeLocal
            if source.mutation_mode == super::model::WorkflowVariableMutationMode::Immutable =>
        {
            Ok(())
        }
        WorkflowVariableScope::Run | WorkflowVariableScope::CompositeLocal => {
            let preceding =
                preceding_assignments(source_assignments, &assignment.writer_step_id, reachable);
            if preceding.is_empty() {
                return if source.default_value_digest.is_some() {
                    Ok(())
                } else {
                    Err(format!(
                        "assignment {:?} reads {:?} before it is assigned",
                        assignment.id, assignment.source_variable
                    ))
                };
            }
            if source.default_value_digest.is_none()
                && !preceding.iter().any(|source_writer| {
                    dominators
                        .get(&assignment.writer_step_id)
                        .is_some_and(|values| values.contains(&source_writer.writer_step_id))
                })
            {
                return Err(format!(
                    "assignment {:?} has no dominating source assignment",
                    assignment.id
                ));
            }
            Ok(())
        }
    }
}

fn preceding_assignments<'a>(
    assignments: Option<&'a [&'a WorkflowVariableAssignment]>,
    consumer: &str,
    reachable: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<&'a WorkflowVariableAssignment> {
    assignments
        .into_iter()
        .flatten()
        .copied()
        .filter(|value| strictly_precedes(&value.writer_step_id, consumer, reachable))
        .collect()
}

fn validate_region_binding(
    region: Option<&str>,
    step: &str,
    steps: &BTreeMap<&str, &super::super::WorkflowStepSpec>,
) -> Result<(), String> {
    let Some(region) = region else {
        return Ok(());
    };
    let region_step = steps
        .get(region)
        .ok_or_else(|| format!("Workflow variable binding references missing region {region:?}"))?;
    if region_step.kind != WorkflowStepKind::Subworkflow || step != region {
        return Err(
            "Composite variable bindings must target their exact composite region step".into(),
        );
    }
    Ok(())
}

fn require_predecessor(
    source: &str,
    consumer: &str,
    required: bool,
    dominators: &BTreeMap<String, BTreeSet<String>>,
    reachable: &BTreeMap<String, BTreeSet<String>>,
) -> Result<(), String> {
    if !strictly_precedes(source, consumer, reachable) {
        return Err(format!(
            "Workflow variable source {source:?} does not precede consumer {consumer:?}"
        ));
    }
    if required
        && !dominators
            .get(consumer)
            .is_some_and(|values| values.contains(source))
    {
        return Err(format!(
            "Required Workflow variable source {source:?} does not dominate consumer {consumer:?}"
        ));
    }
    Ok(())
}

fn strictly_precedes(
    source: &str,
    target: &str,
    reachable: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    source != target
        && reachable
            .get(source)
            .is_some_and(|values| values.contains(target))
}

fn assignments_by_target(
    assignments: &[WorkflowVariableAssignment],
) -> BTreeMap<&str, Vec<&WorkflowVariableAssignment>> {
    let mut values = BTreeMap::<&str, Vec<&WorkflowVariableAssignment>>::new();
    for assignment in assignments {
        values
            .entry(assignment.target_variable.as_str())
            .or_default()
            .push(assignment);
    }
    values
}

fn predecessors<'a>(
    workflow: &'a WorkflowSpec,
    steps: &BTreeMap<&'a str, &'a super::super::WorkflowStepSpec>,
) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let mut values = steps
        .keys()
        .map(|id| ((*id).to_owned(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for edge in &workflow.edges {
        values
            .get_mut(&edge.target)
            .ok_or_else(|| format!("Workflow variable graph lost target {:?}", edge.target))?
            .insert(edge.source.clone());
    }
    Ok(values)
}

fn dominators(
    order: &[String],
    input: &str,
    predecessors: &BTreeMap<String, BTreeSet<String>>,
) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let all = order.iter().cloned().collect::<BTreeSet<_>>();
    let mut values = order
        .iter()
        .map(|id| {
            let initial = if id == input {
                BTreeSet::from([id.clone()])
            } else {
                all.clone()
            };
            (id.clone(), initial)
        })
        .collect::<BTreeMap<_, _>>();
    for id in order.iter().filter(|id| id.as_str() != input) {
        let sources = predecessors
            .get(id)
            .ok_or_else(|| format!("Workflow variable graph lost predecessors for {id:?}"))?;
        let mut intersection = all.clone();
        for source in sources {
            let source_dominators = values
                .get(source)
                .ok_or_else(|| format!("Workflow variable graph lost dominators for {source:?}"))?;
            intersection = intersection
                .intersection(source_dominators)
                .cloned()
                .collect();
        }
        intersection.insert(id.clone());
        values.insert(id.clone(), intersection);
    }
    Ok(values)
}

fn transitive_reachability(
    order: &[String],
    workflow: &WorkflowSpec,
    steps: &BTreeMap<&str, &super::super::WorkflowStepSpec>,
) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let mut outgoing = steps
        .keys()
        .map(|id| ((*id).to_owned(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for edge in &workflow.edges {
        outgoing
            .get_mut(&edge.source)
            .ok_or_else(|| format!("Workflow variable graph lost source {:?}", edge.source))?
            .insert(edge.target.clone());
    }
    let mut reachable = BTreeMap::<String, BTreeSet<String>>::new();
    for id in order.iter().rev() {
        let mut descendants = BTreeSet::new();
        for child in outgoing
            .get(id)
            .ok_or_else(|| format!("Workflow variable graph lost outgoing edges for {id:?}"))?
        {
            descendants.insert(child.clone());
            if let Some(nested) = reachable.get(child) {
                descendants.extend(nested.iter().cloned());
            }
        }
        reachable.insert(id.clone(), descendants);
    }
    Ok(reachable)
}
