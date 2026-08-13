use crate::modules::shared_kernel::domain::{canonical_timestamp, PrincipalId, WorkflowRunId};
use crate::modules::workflow::domain::{
    workflow_run_timeout_seconds, PlanRevision, ResolvedWorkflowPayload, WorkflowGoal,
    WorkflowRevision, WorkflowRun, WorkflowRunInput, WorkflowStepProjection,
    WORKFLOW_PLAN_SCHEMA_V2, WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION,
    WORKFLOW_RUN_INPUT_SCHEMA, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION,
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
        if plan.schema == WORKFLOW_PLAN_SCHEMA_V2 {
            return Err(
                "Workflow plan v2 execution requires the typed-variable Flow adapter".into(),
            );
        }
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
            schema: WORKFLOW_RUN_INPUT_SCHEMA.into(),
            runtime_contract_revision: WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION.into(),
            flow_workflow_name: WORKFLOW_RUN_FLOW_NAME.into(),
            flow_workflow_version: WORKFLOW_RUN_FLOW_VERSION.into(),
            organization_id: goal.organization_id,
            project_id: goal.project_id,
            workflow_run_id,
            workflow_goal_id: goal.id,
            plan_revision_id: plan_revision.id,
            plan_digest: plan_revision.digest.clone(),
            plan: plan.clone(),
            goal_input: goal.contract.spec().input.clone(),
            payloads,
            requested_at,
            deadline_at,
        };
        let (run, steps) = WorkflowRun::create(input, requested_by)?;
        Ok(CompiledWorkflowRun { run, steps })
    }
}
