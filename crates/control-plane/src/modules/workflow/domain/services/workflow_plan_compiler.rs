use crate::modules::shared_kernel::domain::{PlanRevisionId, PrincipalId, WorkflowGoalId};
use crate::modules::workflow::domain::{
    has_agent_failure_route, has_connector_failure_route, has_transform_failure_route,
    OntologyRevision, PlanRevision, WorkflowDefinition, WorkflowGoal, WorkflowGoalContract,
    WorkflowPlan, WorkflowPlanStep, WorkflowRevision, WorkflowStepBindingKind,
    WORKFLOW_PLAN_COMPILER_REVISION, WORKFLOW_PLAN_COMPILER_REVISION_V10,
    WORKFLOW_PLAN_COMPILER_REVISION_V11, WORKFLOW_PLAN_COMPILER_REVISION_V12,
    WORKFLOW_PLAN_COMPILER_REVISION_V2, WORKFLOW_PLAN_COMPILER_REVISION_V3,
    WORKFLOW_PLAN_COMPILER_REVISION_V4, WORKFLOW_PLAN_COMPILER_REVISION_V5,
    WORKFLOW_PLAN_COMPILER_REVISION_V6, WORKFLOW_PLAN_COMPILER_REVISION_V7,
    WORKFLOW_PLAN_COMPILER_REVISION_V8, WORKFLOW_PLAN_COMPILER_REVISION_V9, WORKFLOW_PLAN_SCHEMA,
    WORKFLOW_PLAN_SCHEMA_V10, WORKFLOW_PLAN_SCHEMA_V11, WORKFLOW_PLAN_SCHEMA_V12,
    WORKFLOW_PLAN_SCHEMA_V2, WORKFLOW_PLAN_SCHEMA_V3, WORKFLOW_PLAN_SCHEMA_V4,
    WORKFLOW_PLAN_SCHEMA_V5, WORKFLOW_PLAN_SCHEMA_V6, WORKFLOW_PLAN_SCHEMA_V7,
    WORKFLOW_PLAN_SCHEMA_V8, WORKFLOW_PLAN_SCHEMA_V9,
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
    pub fn compiler_revision(workflow_revision: &WorkflowRevision) -> &'static str {
        if workflow_revision.semantic_contracts.is_some()
            && has_agent_failure_route(workflow_revision.contract.spec())
        {
            WORKFLOW_PLAN_COMPILER_REVISION_V12
        } else if workflow_revision
            .semantic_contracts
            .as_ref()
            .is_some_and(|contracts| {
                contracts.has_composite_failure_route(workflow_revision.contract.spec())
            })
        {
            WORKFLOW_PLAN_COMPILER_REVISION_V11
        } else if workflow_revision
            .semantic_contracts
            .as_ref()
            .is_some_and(|contracts| {
                contracts.has_branch_failure_route(workflow_revision.contract.spec())
            })
        {
            WORKFLOW_PLAN_COMPILER_REVISION_V10
        } else if workflow_revision
            .semantic_contracts
            .as_ref()
            .is_some_and(|contracts| {
                contracts.has_workflow_output_failure_route(workflow_revision.contract.spec())
            })
        {
            WORKFLOW_PLAN_COMPILER_REVISION_V9
        } else if workflow_revision.semantic_contracts.is_some()
            && has_transform_failure_route(workflow_revision.contract.spec())
        {
            WORKFLOW_PLAN_COMPILER_REVISION_V8
        } else if workflow_revision
            .semantic_contracts
            .as_ref()
            .is_some_and(|contracts| {
                contracts.has_application_answer_failure_route(workflow_revision.contract.spec())
            })
        {
            WORKFLOW_PLAN_COMPILER_REVISION_V7
        } else if workflow_revision.semantic_contracts.is_some()
            && workflow_revision
                .semantic_contracts
                .as_ref()
                .is_some_and(|contracts| {
                    contracts
                        .has_application_variable_failure_route(workflow_revision.contract.spec())
                })
        {
            WORKFLOW_PLAN_COMPILER_REVISION_V6
        } else if workflow_revision.semantic_contracts.is_some()
            && has_connector_failure_route(workflow_revision.contract.spec())
        {
            WORKFLOW_PLAN_COMPILER_REVISION_V5
        } else if workflow_revision
            .semantic_contracts
            .as_ref()
            .is_some_and(|contracts| contracts.has_default_output_fallback())
        {
            WORKFLOW_PLAN_COMPILER_REVISION_V4
        } else if workflow_revision.semantic_contracts.is_some()
            && workflow_revision
                .contract
                .spec()
                .has_non_branch_source_handles()
        {
            WORKFLOW_PLAN_COMPILER_REVISION_V3
        } else if workflow_revision.semantic_contracts.is_some() {
            WORKFLOW_PLAN_COMPILER_REVISION_V2
        } else {
            WORKFLOW_PLAN_COMPILER_REVISION
        }
    }

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
        let semantic_contracts = workflow_revision.semantic_contracts.as_ref();
        let application_variable_failure_version = semantic_contracts.is_some_and(|contracts| {
            contracts.has_application_variable_failure_route(workflow_revision.contract.spec())
        });
        let application_answer_failure_version = semantic_contracts.is_some_and(|contracts| {
            contracts.has_application_answer_failure_route(workflow_revision.contract.spec())
        });
        let transform_failure_version = semantic_contracts.is_some()
            && has_transform_failure_route(workflow_revision.contract.spec());
        let workflow_output_failure_version = semantic_contracts.is_some_and(|contracts| {
            contracts.has_workflow_output_failure_route(workflow_revision.contract.spec())
        });
        let branch_failure_version = semantic_contracts.is_some_and(|contracts| {
            contracts.has_branch_failure_route(workflow_revision.contract.spec())
        });
        let composite_failure_version = semantic_contracts.is_some_and(|contracts| {
            contracts.has_composite_failure_route(workflow_revision.contract.spec())
        });
        let agent_failure_version = semantic_contracts.is_some()
            && has_agent_failure_route(workflow_revision.contract.spec());
        let connector_failure_version = semantic_contracts.is_some()
            && has_connector_failure_route(workflow_revision.contract.spec());
        let default_output_version =
            semantic_contracts.is_some_and(|contracts| contracts.has_default_output_fallback());
        let failure_version = semantic_contracts.is_some()
            && (agent_failure_version
                || composite_failure_version
                || branch_failure_version
                || workflow_output_failure_version
                || transform_failure_version
                || application_answer_failure_version
                || application_variable_failure_version
                || connector_failure_version
                || default_output_version
                || workflow_revision
                    .contract
                    .spec()
                    .has_non_branch_source_handles());
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
                    descriptor: semantic_contracts
                        .and_then(|contracts| contracts.descriptor_bindings().resolve(&step.id))
                        .cloned(),
                    failure: if failure_version {
                        Some(
                            semantic_contracts
                                .ok_or_else(|| {
                                    "Workflow failure route lost semantic contracts".to_owned()
                                })?
                                .failure_contract(&step.id)?
                                .clone(),
                        )
                    } else {
                        None
                    },
                    default_output: if default_output_version {
                        semantic_contracts
                            .ok_or_else(|| {
                                "Workflow default output lost semantic contracts".to_owned()
                            })?
                            .default_output_contract(&step.id)?
                    } else {
                        None
                    },
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
                schema: if agent_failure_version {
                    WORKFLOW_PLAN_SCHEMA_V12
                } else if composite_failure_version {
                    WORKFLOW_PLAN_SCHEMA_V11
                } else if branch_failure_version {
                    WORKFLOW_PLAN_SCHEMA_V10
                } else if workflow_output_failure_version {
                    WORKFLOW_PLAN_SCHEMA_V9
                } else if transform_failure_version {
                    WORKFLOW_PLAN_SCHEMA_V8
                } else if application_answer_failure_version {
                    WORKFLOW_PLAN_SCHEMA_V7
                } else if application_variable_failure_version {
                    WORKFLOW_PLAN_SCHEMA_V6
                } else if connector_failure_version {
                    WORKFLOW_PLAN_SCHEMA_V5
                } else if default_output_version {
                    WORKFLOW_PLAN_SCHEMA_V4
                } else if failure_version {
                    WORKFLOW_PLAN_SCHEMA_V3
                } else if semantic_contracts.is_some() {
                    WORKFLOW_PLAN_SCHEMA_V2
                } else {
                    WORKFLOW_PLAN_SCHEMA
                }
                .into(),
                compiler_revision: Self::compiler_revision(workflow_revision).into(),
                workflow_definition_id: workflow_definition.id,
                workflow_revision_id: workflow_revision.id,
                workflow_digest: workflow_revision.contract.digest().clone(),
                workflow_payload_set_digest: workflow_revision.payload_set_digest.clone(),
                semantic_contract_set_digest: semantic_contracts
                    .map(|contracts| contracts.digest().clone()),
                variable_contract_digest: semantic_contracts
                    .map(|contracts| contracts.variable_contract().digest().clone()),
                composite_regions_digest: semantic_contracts
                    .and_then(|contracts| contracts.composite_regions())
                    .map(|regions| regions.digest().clone()),
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
    if spec.environment_id.is_none()
        && workflow_revision
            .semantic_contracts
            .as_ref()
            .is_some_and(|contracts| {
                contracts.requires_binding(WorkflowStepBindingKind::PlacementPolicy)
            })
    {
        return Err(
            "Workflow descriptors with placement-policy bindings require one exact Goal environment"
                .into(),
        );
    }
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
    workflow_revision.validate_runtime_dispatch_support()?;
    Ok(())
}
