use crate::modules::shared_kernel::domain::{PlanRevisionId, PrincipalId, WorkflowGoalId};
use crate::modules::workflow::domain::{
    OntologyRevision, PlanRevision, WorkflowDefinition, WorkflowGoal, WorkflowGoalContract,
    WorkflowPlan, WorkflowPlanStep, WorkflowRevision, WORKFLOW_PLAN_COMPILER_REVISION,
    WORKFLOW_PLAN_SCHEMA,
};
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledWorkflowGoal {
    pub goal: WorkflowGoal,
    pub plan_revision: PlanRevision,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WorkflowPlanCompiler;

impl WorkflowPlanCompiler {
    #[allow(clippy::too_many_arguments)]
    pub fn compile_goal(
        goal_id: WorkflowGoalId,
        plan_revision_id: PlanRevisionId,
        goal_contract: WorkflowGoalContract,
        workflow_definition: &WorkflowDefinition,
        workflow_revision: &WorkflowRevision,
        ontology_revision: &OntologyRevision,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<CompiledWorkflowGoal, String> {
        validate_authorities(
            &goal_contract,
            workflow_definition,
            workflow_revision,
            ontology_revision,
        )?;
        let order = workflow_revision
            .contract
            .spec()
            .topological_order(Default::default())?;
        let by_id = workflow_revision
            .contract
            .spec()
            .steps
            .iter()
            .map(|step| (step.id.as_str(), step))
            .collect::<BTreeMap<_, _>>();
        let steps = order
            .iter()
            .map(|id| {
                let step = by_id.get(id.as_str()).ok_or_else(|| {
                    format!("Workflow compiler lost step {id:?} after graph validation")
                })?;
                Ok(WorkflowPlanStep {
                    id: step.id.clone(),
                    kind: step.kind,
                    configuration_digest: step.configuration_digest.clone(),
                    input_schema_digest: step.input_schema_digest.clone(),
                    output_schema_digest: step.output_schema_digest.clone(),
                    policy_digest: step.policy_digest.clone(),
                    capability: step.capability.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut edges = workflow_revision.contract.spec().edges.clone();
        edges.sort_by(|left, right| left.id.cmp(&right.id));
        let goal_spec = goal_contract.spec();
        let plan_revision = PlanRevision::create(
            workflow_definition.organization_id,
            workflow_definition.project_id,
            goal_id,
            plan_revision_id,
            WorkflowPlan {
                schema: WORKFLOW_PLAN_SCHEMA.into(),
                compiler_revision: WORKFLOW_PLAN_COMPILER_REVISION.into(),
                workflow_definition_id: workflow_definition.id,
                workflow_revision_id: workflow_revision.id,
                workflow_digest: workflow_revision.contract.digest().clone(),
                workflow_payload_set_digest: workflow_revision.payload_set_digest.clone(),
                ontology_id: ontology_revision.ontology_id,
                ontology_revision_id: ontology_revision.id,
                ontology_digest: ontology_revision.contract.digest().clone(),
                environment_id: goal_spec.environment_id,
                input_digest: goal_contract.input_digest().clone(),
                steps,
                edges,
            },
            created_by,
            created_at,
        )?;
        let goal = WorkflowGoal::create(
            workflow_definition.organization_id,
            workflow_definition.project_id,
            goal_id,
            goal_contract,
            &plan_revision,
            created_by,
            created_at,
        )?;
        Ok(CompiledWorkflowGoal {
            goal,
            plan_revision,
        })
    }
}

fn validate_authorities(
    goal: &WorkflowGoalContract,
    workflow_definition: &WorkflowDefinition,
    workflow_revision: &WorkflowRevision,
    ontology_revision: &OntologyRevision,
) -> Result<(), String> {
    workflow_definition.validate()?;
    workflow_revision.validate()?;
    ontology_revision.validate()?;
    let spec = goal.spec();
    if workflow_revision.organization_id != workflow_definition.organization_id
        || workflow_revision.project_id != workflow_definition.project_id
        || workflow_revision.workflow_definition_id != workflow_definition.id
        || ontology_revision.organization_id != workflow_definition.organization_id
        || ontology_revision.project_id != workflow_definition.project_id
        || spec.workflow_definition_id != workflow_definition.id
        || spec.workflow_revision_id != workflow_revision.id
        || &spec.workflow_digest != workflow_revision.contract.digest()
        || spec.ontology_id != ontology_revision.ontology_id
        || spec.ontology_revision_id != ontology_revision.id
        || &spec.ontology_digest != ontology_revision.contract.digest()
    {
        return Err(
            "Workflow goal authority references do not match the exact admitted revisions".into(),
        );
    }
    Ok(())
}
