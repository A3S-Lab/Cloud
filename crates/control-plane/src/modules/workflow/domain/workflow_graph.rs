use super::validation::{validate_identifier, validate_text};
use super::{WorkflowContractQuotas, WorkflowEdgeSpec, WorkflowSpec, WorkflowStepKind};
use a3s_flow::{WorkflowDag, WorkflowDagEdge, WorkflowDagNode};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) fn validate_workflow(
    spec: &WorkflowSpec,
    quotas: WorkflowContractQuotas,
) -> Result<Vec<String>, String> {
    validate_text("Workflow name", &spec.name, 1, 120)?;
    validate_text("Workflow description", &spec.description, 0, 4_096)?;
    if spec.steps.len() < 2 || spec.steps.len() > quotas.max_steps {
        return Err(format!(
            "Workflow must contain between 2 and {} steps",
            quotas.max_steps
        ));
    }
    if spec.edges.is_empty() || spec.edges.len() > quotas.max_edges {
        return Err(format!(
            "Workflow must contain between 1 and {} edges",
            quotas.max_edges
        ));
    }

    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    for step in &spec.steps {
        validate_identifier("Workflow step ID", &step.id)?;
        validate_text("Workflow step label", &step.label, 1, 120)?;
        validate_capability_binding(step.kind, step.capability.as_ref())?;
        match step.kind {
            WorkflowStepKind::Input => inputs.push(step.id.as_str()),
            WorkflowStepKind::Output => outputs.push(step.id.as_str()),
            _ => {}
        }
    }
    for edge in &spec.edges {
        validate_identifier("Workflow edge ID", &edge.id)?;
    }

    let structural_order = compile_structural_order(spec)?;

    let steps = spec
        .steps
        .iter()
        .map(|step| (step.id.as_str(), step))
        .collect::<BTreeMap<_, _>>();
    if inputs.len() != 1 || outputs.is_empty() {
        return Err(format!(
            "Workflow must contain exactly one input and at least one output step; found {} input and {} output steps",
            inputs.len(),
            outputs.len()
        ));
    }

    let input = inputs[0];
    let mut outgoing: BTreeMap<&str, Vec<&str>> =
        steps.keys().copied().map(|id| (id, Vec::new())).collect();
    let mut incoming: BTreeMap<&str, Vec<&str>> =
        steps.keys().copied().map(|id| (id, Vec::new())).collect();
    let mut branch_handles: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();

    for edge in &spec.edges {
        validate_edge_semantics(edge, &steps, &mut branch_handles)?;
        outgoing
            .get_mut(edge.source.as_str())
            .ok_or_else(|| format!("Workflow structural plan lost source {:?}", edge.source))?
            .push(edge.target.as_str());
        incoming
            .get_mut(edge.target.as_str())
            .ok_or_else(|| format!("Workflow structural plan lost target {:?}", edge.target))?
            .push(edge.source.as_str());
    }

    if !incoming[input].is_empty() {
        return Err("Workflow input step cannot have incoming edges".into());
    }
    for output in &outputs {
        if !outgoing[output].is_empty() {
            return Err(format!(
                "Workflow output step {output:?} cannot have outgoing edges"
            ));
        }
    }
    for id in steps.keys().copied() {
        if id != input && incoming[id].is_empty() {
            return Err(format!("Workflow step {id:?} has no upstream step"));
        }
        if !outputs.contains(&id) && outgoing[id].is_empty() {
            return Err(format!("Workflow step {id:?} does not lead toward output"));
        }
    }

    if walk(input, &outgoing).len() != steps.len() {
        return Err("Every Workflow step must be reachable from input".into());
    }
    let mut reaches_output = BTreeSet::new();
    for output in outputs {
        reaches_output.extend(walk(output, &incoming));
    }
    if reaches_output.len() != steps.len() {
        return Err("Every Workflow step must lead to output".into());
    }

    Ok(structural_order)
}

/// Translate Cloud's authoritative ACL-backed product graph into Flow's pure
/// structural compiler input. Flow owns generic graph identities, endpoints,
/// scopes, acyclicity, and deterministic ordering; no external document parser
/// is used in Cloud.
fn compile_structural_order(spec: &WorkflowSpec) -> Result<Vec<String>, String> {
    WorkflowDag::new(
        spec.steps
            .iter()
            .map(|step| WorkflowDagNode::new(&step.id, step.kind.as_str()))
            .collect(),
        spec.edges
            .iter()
            .map(|edge| {
                let compiled = WorkflowDagEdge::new(&edge.id, &edge.source, &edge.target);
                match &edge.source_handle {
                    Some(handle) => compiled.with_source_handle(handle),
                    None => compiled,
                }
            })
            .collect(),
    )
    .execution_plan()
    .map(|plan| plan.top_level().to_vec())
    .map_err(|error| format!("Workflow graph structure is invalid: {error}"))
}

fn validate_capability_binding(
    kind: WorkflowStepKind,
    capability: Option<&super::CapabilityReference>,
) -> Result<(), String> {
    let allowed = kind.allowed_capability_types();
    match (allowed, capability) {
        ([], None) => Ok(()),
        ([], Some(_)) => Err(format!(
            "Workflow {} steps cannot bind an external capability",
            kind.as_str()
        )),
        (_, None) => Err(format!(
            "Workflow {} steps require one exact capability reference",
            kind.as_str()
        )),
        (allowed, Some(reference)) => {
            reference.validate()?;
            if allowed.contains(&reference.capability_type) {
                Ok(())
            } else {
                Err(format!(
                    "Workflow {} step cannot bind capability type {}",
                    kind.as_str(),
                    reference.capability_type.as_str()
                ))
            }
        }
    }
}

fn validate_edge_semantics<'a>(
    edge: &'a WorkflowEdgeSpec,
    steps: &BTreeMap<&str, &super::WorkflowStepSpec>,
    branch_handles: &mut BTreeMap<&'a str, BTreeSet<&'a str>>,
) -> Result<(), String> {
    let source = steps.get(edge.source.as_str()).ok_or_else(|| {
        format!(
            "Workflow edge {:?} references missing source {:?}",
            edge.id, edge.source
        )
    })?;
    match (source.kind, edge.source_handle.as_deref()) {
        (WorkflowStepKind::Branch, Some(handle)) => {
            validate_identifier("Workflow branch handle", handle)?;
            if !branch_handles
                .entry(edge.source.as_str())
                .or_default()
                .insert(handle)
            {
                return Err(format!(
                    "Workflow branch {:?} contains duplicate handle {handle:?}",
                    edge.source
                ));
            }
        }
        (WorkflowStepKind::Branch, None) => {
            return Err(format!(
                "Workflow branch edge {:?} requires source_handle",
                edge.id
            ));
        }
        (_, Some(_)) => {
            return Err(format!(
                "Workflow non-branch edge {:?} cannot declare source_handle",
                edge.id
            ));
        }
        (_, None) => {}
    }
    Ok(())
}

fn walk<'a>(start: &'a str, adjacency: &BTreeMap<&'a str, Vec<&'a str>>) -> BTreeSet<&'a str> {
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::from([start]);
    while let Some(id) = queue.pop_front() {
        if !visited.insert(id) {
            continue;
        }
        if let Some(next) = adjacency.get(id) {
            queue.extend(next.iter().copied());
        }
    }
    visited
}
