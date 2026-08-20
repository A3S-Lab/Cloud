use crate::modules::shared_kernel::domain::{canonical_timestamp, PrincipalId, WorkflowRunId};
use crate::modules::workflow::domain::{
    validate_runtime_variable_contract, workflow_run_timeout_seconds, PlanRevision,
    ResolvedWorkflowCompositeRegions, ResolvedWorkflowPayload, ResolvedWorkflowVariableContract,
    ResolvedWorkflowVariableDefaults, WorkflowGoal, WorkflowRevision, WorkflowRun,
    WorkflowRunInput, WorkflowStepProjection, WORKFLOW_PLAN_SCHEMA, WORKFLOW_PLAN_SCHEMA_V2,
    WORKFLOW_PLAN_SCHEMA_V3, WORKFLOW_PLAN_SCHEMA_V4, WORKFLOW_RUN_FLOW_NAME,
    WORKFLOW_RUN_FLOW_VERSION, WORKFLOW_RUN_FLOW_VERSION_V2, WORKFLOW_RUN_FLOW_VERSION_V3,
    WORKFLOW_RUN_FLOW_VERSION_V4, WORKFLOW_RUN_FLOW_VERSION_V7, WORKFLOW_RUN_FLOW_VERSION_V8,
    WORKFLOW_RUN_INPUT_SCHEMA, WORKFLOW_RUN_INPUT_SCHEMA_V2, WORKFLOW_RUN_INPUT_SCHEMA_V3,
    WORKFLOW_RUN_INPUT_SCHEMA_V4, WORKFLOW_RUN_INPUT_SCHEMA_V7, WORKFLOW_RUN_INPUT_SCHEMA_V8,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V3, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V4,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V7, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8,
};
use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledWorkflowRun {
    pub run: WorkflowRun,
    pub steps: Vec<WorkflowStepProjection>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WorkflowRunCompiler;

impl WorkflowRunCompiler {
    #[allow(clippy::too_many_arguments)]
    pub fn compile(
        workflow_run_id: WorkflowRunId,
        goal: &WorkflowGoal,
        plan_revision: &PlanRevision,
        workflow_revision: &WorkflowRevision,
        timeout_seconds: Option<u64>,
        requested_by: PrincipalId,
        requested_at: DateTime<Utc>,
    ) -> Result<CompiledWorkflowRun, String> {
        goal.validate(plan_revision)?;
        workflow_revision.validate()?;
        let plan = &plan_revision.plan;
        if workflow_run_id.as_uuid().is_nil()
            || requested_by.as_uuid().is_nil()
            || goal.organization_id != plan_revision.organization_id
            || goal.project_id != plan_revision.project_id
            || goal.id != plan_revision.workflow_goal_id
            || goal.plan_revision_id != plan_revision.id
            || goal.plan_digest != plan_revision.digest
            || workflow_revision.organization_id != goal.organization_id
            || workflow_revision.project_id != goal.project_id
            || workflow_revision.workflow_definition_id != plan.workflow_definition_id
            || workflow_revision.id != plan.workflow_revision_id
            || workflow_revision.contract.digest() != &plan.workflow_digest
            || workflow_revision.payload_set_digest != plan.workflow_payload_set_digest
            || goal.contract.input_digest() != &plan.input_digest
        {
            return Err(
                "WorkflowRun authorities do not match the exact Goal, Plan, and Workflow revision"
                    .into(),
            );
        }
        let (
            input_schema,
            runtime_revision,
            flow_version,
            variable_contract,
            variable_defaults,
            composite_regions,
        ) = match (
            plan.schema.as_str(),
            workflow_revision.semantic_contracts.as_ref(),
        ) {
            (WORKFLOW_PLAN_SCHEMA, None) => (
                WORKFLOW_RUN_INPUT_SCHEMA,
                WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION,
                WORKFLOW_RUN_FLOW_VERSION,
                None,
                None,
                None,
            ),
            (WORKFLOW_PLAN_SCHEMA_V2, Some(contracts)) => {
                contracts.validate_plan_bindings(plan)?;
                validate_runtime_variable_contract(
                    contracts.variable_contract(),
                    contracts.variable_defaults(),
                    plan,
                )?;
                let composite_regions = contracts
                    .composite_regions()
                    .map(ResolvedWorkflowCompositeRegions::from_regions);
                let (input_schema, runtime_revision, flow_version) =
                    plan_v2_runtime_contract(plan, composite_regions.is_some());
                (
                    input_schema,
                    runtime_revision,
                    flow_version,
                    Some(ResolvedWorkflowVariableContract::from_contract(
                        contracts.variable_contract(),
                    )),
                    contracts
                        .variable_defaults()
                        .map(ResolvedWorkflowVariableDefaults::from_defaults),
                    composite_regions,
                )
            }
            (WORKFLOW_PLAN_SCHEMA_V3, Some(contracts)) => {
                contracts.validate_plan_bindings(plan)?;
                validate_runtime_variable_contract(
                    contracts.variable_contract(),
                    contracts.variable_defaults(),
                    plan,
                )?;
                let (input_schema, runtime_revision, flow_version) = plan_v3_runtime_contract(plan);
                (
                    input_schema,
                    runtime_revision,
                    flow_version,
                    Some(ResolvedWorkflowVariableContract::from_contract(
                        contracts.variable_contract(),
                    )),
                    contracts
                        .variable_defaults()
                        .map(ResolvedWorkflowVariableDefaults::from_defaults),
                    contracts
                        .composite_regions()
                        .map(ResolvedWorkflowCompositeRegions::from_regions),
                )
            }
            (WORKFLOW_PLAN_SCHEMA_V4, Some(contracts)) => {
                contracts.validate_plan_bindings(plan)?;
                validate_runtime_variable_contract(
                    contracts.variable_contract(),
                    contracts.variable_defaults(),
                    plan,
                )?;
                let (input_schema, runtime_revision, flow_version) = plan_v4_runtime_contract(plan);
                (
                    input_schema,
                    runtime_revision,
                    flow_version,
                    Some(ResolvedWorkflowVariableContract::from_contract(
                        contracts.variable_contract(),
                    )),
                    contracts
                        .variable_defaults()
                        .map(ResolvedWorkflowVariableDefaults::from_defaults),
                    contracts
                        .composite_regions()
                        .map(ResolvedWorkflowCompositeRegions::from_regions),
                )
            }
            _ => {
                return Err(
                    "WorkflowRun plan version does not match its Workflow semantic authority"
                        .into(),
                )
            }
        };
        let timeout_seconds = workflow_run_timeout_seconds(timeout_seconds)?;
        let requested_at = canonical_timestamp(requested_at);
        let timeout_seconds = i64::try_from(timeout_seconds)
            .map_err(|_| "WorkflowRun timeout exceeds the supported duration".to_owned())?;
        let deadline_at = requested_at
            .checked_add_signed(Duration::seconds(timeout_seconds))
            .ok_or_else(|| "WorkflowRun deadline overflowed".to_owned())?;
        let mut payloads = workflow_revision
            .payloads
            .iter()
            .map(ResolvedWorkflowPayload::from_payload)
            .collect::<Vec<_>>();
        payloads.sort_by(|left, right| left.digest.cmp(&right.digest));
        let input = WorkflowRunInput {
            schema: input_schema.into(),
            runtime_contract_revision: runtime_revision.into(),
            flow_workflow_name: WORKFLOW_RUN_FLOW_NAME.into(),
            flow_workflow_version: flow_version.into(),
            organization_id: goal.organization_id,
            project_id: goal.project_id,
            workflow_run_id,
            workflow_goal_id: goal.id,
            plan_revision_id: plan_revision.id,
            plan_digest: plan_revision.digest.clone(),
            plan: plan.clone(),
            goal_input: goal.contract.spec().input.clone(),
            payloads,
            variable_contract,
            variable_defaults,
            composite_regions,
            requested_at,
            deadline_at,
        };
        let (run, steps) = WorkflowRun::create(input, requested_by)?;
        Ok(CompiledWorkflowRun { run, steps })
    }
}

fn plan_v2_runtime_contract(
    plan: &crate::modules::workflow::domain::WorkflowPlan,
    has_composite_regions: bool,
) -> (&'static str, &'static str, &'static str) {
    if plan_has_connector(plan) {
        (
            WORKFLOW_RUN_INPUT_SCHEMA_V8,
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8,
            WORKFLOW_RUN_FLOW_VERSION_V8,
        )
    } else if has_composite_regions {
        (
            WORKFLOW_RUN_INPUT_SCHEMA_V3,
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V3,
            WORKFLOW_RUN_FLOW_VERSION_V3,
        )
    } else {
        (
            WORKFLOW_RUN_INPUT_SCHEMA_V2,
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2,
            WORKFLOW_RUN_FLOW_VERSION_V2,
        )
    }
}

fn plan_v3_runtime_contract(
    plan: &crate::modules::workflow::domain::WorkflowPlan,
) -> (&'static str, &'static str, &'static str) {
    if plan_has_connector(plan) {
        (
            WORKFLOW_RUN_INPUT_SCHEMA_V8,
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8,
            WORKFLOW_RUN_FLOW_VERSION_V8,
        )
    } else {
        (
            WORKFLOW_RUN_INPUT_SCHEMA_V4,
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V4,
            WORKFLOW_RUN_FLOW_VERSION_V4,
        )
    }
}

fn plan_v4_runtime_contract(
    plan: &crate::modules::workflow::domain::WorkflowPlan,
) -> (&'static str, &'static str, &'static str) {
    if plan_has_connector(plan) {
        (
            WORKFLOW_RUN_INPUT_SCHEMA_V8,
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8,
            WORKFLOW_RUN_FLOW_VERSION_V8,
        )
    } else {
        (
            WORKFLOW_RUN_INPUT_SCHEMA_V7,
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V7,
            WORKFLOW_RUN_FLOW_VERSION_V7,
        )
    }
}

fn plan_has_connector(plan: &crate::modules::workflow::domain::WorkflowPlan) -> bool {
    plan.steps.iter().any(|step| {
        step.capability.as_ref().is_some_and(|capability| {
            capability.capability_type
                == crate::modules::workflow::domain::CapabilityType::ConnectorRevision
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_connector_binding_selects_v8_even_with_composite_regions() {
        let input = crate::modules::workflow::test_support::connector_workflow_run_input()
            .expect("Connector WorkflowRun input");
        assert_eq!(
            plan_v2_runtime_contract(&input.plan, false),
            (
                WORKFLOW_RUN_INPUT_SCHEMA_V8,
                WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8,
                WORKFLOW_RUN_FLOW_VERSION_V8,
            )
        );
        assert_eq!(
            plan_v2_runtime_contract(&input.plan, true),
            (
                WORKFLOW_RUN_INPUT_SCHEMA_V8,
                WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8,
                WORKFLOW_RUN_FLOW_VERSION_V8,
            )
        );
        assert_eq!(
            plan_v3_runtime_contract(&input.plan),
            (
                WORKFLOW_RUN_INPUT_SCHEMA_V8,
                WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8,
                WORKFLOW_RUN_FLOW_VERSION_V8,
            )
        );
        assert_eq!(
            plan_v4_runtime_contract(&input.plan),
            (
                WORKFLOW_RUN_INPUT_SCHEMA_V8,
                WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8,
                WORKFLOW_RUN_FLOW_VERSION_V8,
            )
        );
    }
}
