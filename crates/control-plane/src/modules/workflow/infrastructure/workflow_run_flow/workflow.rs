use super::types::{
    WorkflowLocalStepInput, WorkflowLocalStepResult, WorkflowRunOutput,
    WORKFLOW_LOCAL_STEP_INPUT_SCHEMA, WORKFLOW_LOCAL_STEP_NAME, WORKFLOW_RUN_OUTPUT_SCHEMA,
};
use crate::modules::workflow::domain::{
    PlanRevision, WorkflowDataSchema, WorkflowEdgeSpec, WorkflowPayloadContent,
    WorkflowPayloadKind, WorkflowRevision, WorkflowRun, WorkflowStepConfiguration,
    WorkflowStepKind,
};
use a3s_flow::{FlowError, RuntimeCommand, StepCommand, WorkflowInvocation};
use std::collections::BTreeMap;

enum ResolvedStep {
    Active(WorkflowLocalStepResult),
    Inactive,
}

pub(super) fn replay(
    invocation: WorkflowInvocation,
    run: &WorkflowRun,
    plan: &PlanRevision,
    revision: &WorkflowRevision,
    workflow_input: &serde_json::Value,
) -> a3s_flow::Result<RuntimeCommand> {
    let context = invocation.context();
    let incoming = incoming_edges(&plan.plan.edges, &plan.plan.steps);
    let kinds = plan
        .plan
        .steps
        .iter()
        .map(|step| (step.id.as_str(), step.kind))
        .collect::<BTreeMap<_, _>>();
    let mut resolved = BTreeMap::<String, ResolvedStep>::new();
    let mut ready = Vec::new();

    for step in &plan.plan.steps {
        let Some(dependencies) = dependency_state(step.id.as_str(), &incoming, &resolved, &kinds)?
        else {
            continue;
        };
        let Some(dependencies) = dependencies else {
            resolved.insert(step.id.clone(), ResolvedStep::Inactive);
            continue;
        };
        let step_id = flow_step_id(&step.id);
        if let Some(error) = context.step_failed(&step_id) {
            return Ok(context.fail(format!("Workflow step {:?} failed: {error}", step.id)));
        }
        let prepared = prepare_step(run, plan, revision, step, workflow_input, dependencies)?;
        if let Some(result) = context.step_output_as::<WorkflowLocalStepResult>(&step_id)? {
            result
                .validate(step, &prepared.output_schema)
                .map_err(FlowError::Runtime)?;
            if step.kind == WorkflowStepKind::Branch {
                validate_route(step.id.as_str(), result.route.as_deref(), &plan.plan.edges)?;
            }
            if step.kind == WorkflowStepKind::Output {
                return Ok(context.complete(serde_json::to_value(WorkflowRunOutput {
                    schema: WORKFLOW_RUN_OUTPUT_SCHEMA.into(),
                    workflow_run_id: run.id,
                    plan_revision_id: plan.id,
                    plan_digest: plan.digest.to_string(),
                    output: result.output,
                })?));
            }
            resolved.insert(step.id.clone(), ResolvedStep::Active(result));
            continue;
        }
        ready.push(StepCommand::new(
            step_id,
            WORKFLOW_LOCAL_STEP_NAME,
            serde_json::to_value(prepared)?,
        ));
    }

    if ready.is_empty() {
        Ok(context.fail("Workflow plan stalled before its output step completed"))
    } else {
        Ok(context.schedule_steps(ready))
    }
}

fn incoming_edges<'a>(
    edges: &'a [WorkflowEdgeSpec],
    steps: &[crate::modules::workflow::domain::WorkflowPlanStep],
) -> BTreeMap<String, Vec<&'a WorkflowEdgeSpec>> {
    let mut incoming = steps
        .iter()
        .map(|step| (step.id.clone(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for edge in edges {
        if let Some(values) = incoming.get_mut(edge.target.as_str()) {
            values.push(edge);
            values.sort_by(|left, right| left.id.cmp(&right.id));
        }
    }
    incoming
}

fn dependency_state(
    step_id: &str,
    incoming: &BTreeMap<String, Vec<&WorkflowEdgeSpec>>,
    resolved: &BTreeMap<String, ResolvedStep>,
    kinds: &BTreeMap<&str, WorkflowStepKind>,
) -> a3s_flow::Result<Option<Option<BTreeMap<String, serde_json::Value>>>> {
    let edges = incoming.get(step_id).ok_or_else(|| {
        FlowError::InvalidWorkflow(format!("Workflow step {step_id:?} has no edge state"))
    })?;
    if edges.is_empty() {
        return Ok(Some(Some(BTreeMap::new())));
    }
    let mut dependencies = BTreeMap::new();
    let mut active = false;
    for edge in edges {
        let Some(source) = resolved.get(&edge.source) else {
            return Ok(None);
        };
        let ResolvedStep::Active(result) = source else {
            continue;
        };
        let source_kind = kinds.get(edge.source.as_str()).ok_or_else(|| {
            FlowError::InvalidWorkflow(format!("Workflow edge {:?} has no source step", edge.id))
        })?;
        let selected = if *source_kind == WorkflowStepKind::Branch {
            result.route.as_deref() == edge.source_handle.as_deref()
        } else {
            true
        };
        if selected {
            active = true;
            dependencies.insert(edge.source.clone(), result.output.clone());
        }
    }
    if active {
        Ok(Some(Some(dependencies)))
    } else {
        Ok(Some(None))
    }
}

fn prepare_step(
    run: &WorkflowRun,
    plan: &PlanRevision,
    revision: &WorkflowRevision,
    step: &crate::modules::workflow::domain::WorkflowPlanStep,
    workflow_input: &serde_json::Value,
    dependencies: BTreeMap<String, serde_json::Value>,
) -> a3s_flow::Result<WorkflowLocalStepInput> {
    let configuration = configuration(revision, step.configuration_digest.as_str())?;
    let input_schema = data_schema(revision, step.input_schema_digest.as_str())?;
    let output_schema = data_schema(revision, step.output_schema_digest.as_str())?;
    Ok(WorkflowLocalStepInput {
        schema: WORKFLOW_LOCAL_STEP_INPUT_SCHEMA.into(),
        workflow_run_id: run.id,
        plan_revision_id: plan.id,
        plan_digest: plan.digest.to_string(),
        step: step.clone(),
        configuration,
        input_schema,
        output_schema,
        workflow_input: workflow_input.clone(),
        dependencies,
    })
}

fn configuration(
    revision: &WorkflowRevision,
    digest: &str,
) -> a3s_flow::Result<WorkflowStepConfiguration> {
    let payload = revision
        .payloads
        .iter()
        .find(|payload| {
            payload.kind() == WorkflowPayloadKind::Configuration
                && payload.digest().as_str() == digest
        })
        .ok_or_else(|| {
            FlowError::InvalidWorkflow(format!(
                "Workflow configuration payload {digest} is missing"
            ))
        })?;
    match payload.content() {
        WorkflowPayloadContent::Configuration(value) => Ok(value.clone()),
        _ => Err(FlowError::InvalidWorkflow(
            "Workflow configuration payload has another kind".into(),
        )),
    }
}

fn data_schema(revision: &WorkflowRevision, digest: &str) -> a3s_flow::Result<WorkflowDataSchema> {
    let payload = revision
        .payloads
        .iter()
        .find(|payload| {
            payload.kind() == WorkflowPayloadKind::DataSchema && payload.digest().as_str() == digest
        })
        .ok_or_else(|| {
            FlowError::InvalidWorkflow(format!("Workflow data schema {digest} is missing"))
        })?;
    match payload.content() {
        WorkflowPayloadContent::DataSchema(value) => Ok(value.clone()),
        _ => Err(FlowError::InvalidWorkflow(
            "Workflow data-schema payload has another kind".into(),
        )),
    }
}

fn validate_route(
    step_id: &str,
    route: Option<&str>,
    edges: &[WorkflowEdgeSpec],
) -> a3s_flow::Result<()> {
    let route = route.ok_or_else(|| {
        FlowError::Runtime(format!("Workflow branch {step_id:?} omitted its route"))
    })?;
    if edges
        .iter()
        .any(|edge| edge.source == step_id && edge.source_handle.as_deref() == Some(route))
    {
        Ok(())
    } else {
        Err(FlowError::Runtime(format!(
            "Workflow branch {step_id:?} selected unknown route {route:?}"
        )))
    }
}

fn flow_step_id(step_id: &str) -> String {
    format!("workflow-step:{step_id}")
}
