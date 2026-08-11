mod local_step;
mod types;
mod workflow;

use crate::modules::shared_kernel::domain::{Sha256Digest, WorkflowRunId};
use crate::modules::workflow::application::{
    WorkflowRunOperationInput, WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION,
};
use crate::modules::workflow::domain::{
    validate_locally_executable_plan, IWorkflowDefinitionRepository, IWorkflowGoalRepository,
    IWorkflowRunRepository,
};
use a3s_flow::{FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation};
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

pub(crate) use types::WORKFLOW_LOCAL_STEP_NAME;
pub use types::{WorkflowLocalStepResult, WorkflowRunOutput};

#[derive(Clone)]
pub struct WorkflowRunFlowRuntime {
    runs: Arc<dyn IWorkflowRunRepository>,
    goals: Arc<dyn IWorkflowGoalRepository>,
    definitions: Arc<dyn IWorkflowDefinitionRepository>,
}

impl WorkflowRunFlowRuntime {
    pub fn new(
        runs: Arc<dyn IWorkflowRunRepository>,
        goals: Arc<dyn IWorkflowGoalRepository>,
        definitions: Arc<dyn IWorkflowDefinitionRepository>,
    ) -> Self {
        Self {
            runs,
            goals,
            definitions,
        }
    }
}

#[async_trait]
impl FlowRuntime for WorkflowRunFlowRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if invocation.spec.name != WORKFLOW_RUN_FLOW_NAME
            || invocation.spec.version != WORKFLOW_RUN_FLOW_VERSION
        {
            return Err(FlowError::Runtime(format!(
                "Cloud has no WorkflowRun runtime for {}@{}",
                invocation.spec.name, invocation.spec.version
            )));
        }
        let input = invocation.input_as::<WorkflowRunOperationInput>()?;
        let operation_id = Uuid::parse_str(&invocation.run_id).map_err(|error| {
            FlowError::Runtime(format!(
                "WorkflowRun Operation identity is invalid: {error}"
            ))
        })?;
        let run = self
            .runs
            .find(
                input.organization_id,
                WorkflowRunId::from_uuid(operation_id),
            )
            .await
            .map_err(repository_error)?
            .ok_or_else(|| FlowError::Runtime("WorkflowRun is missing".into()))?;
        input.validate_against(&run).map_err(FlowError::Runtime)?;
        if run.operation_id.as_uuid() != operation_id {
            return Err(FlowError::Runtime(
                "A3S Flow run does not match the WorkflowRun Operation".into(),
            ));
        }
        let goal = self
            .goals
            .find(run.organization_id, run.workflow_goal_id)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| FlowError::Runtime("WorkflowRun Goal is missing".into()))?;
        run.validate_against(&goal.goal, &goal.plan_revision)
            .map_err(FlowError::Runtime)?;
        validate_locally_executable_plan(&goal.plan_revision.plan).map_err(FlowError::Runtime)?;
        let plan = &goal.plan_revision;
        let revision = self
            .definitions
            .find_revision(
                run.organization_id,
                plan.plan.workflow_definition_id,
                plan.plan.workflow_revision_id,
            )
            .await
            .map_err(repository_error)?
            .ok_or_else(|| FlowError::Runtime("WorkflowRun revision is missing".into()))?;
        revision.validate().map_err(FlowError::Runtime)?;
        if revision.organization_id != run.organization_id
            || revision.project_id != run.project_id
            || revision.workflow_definition_id != plan.plan.workflow_definition_id
            || revision.id != plan.plan.workflow_revision_id
            || revision.contract.digest() != &plan.plan.workflow_digest
            || revision.payload_set_digest != plan.plan.workflow_payload_set_digest
            || Sha256Digest::parse(&input.plan_digest).map_err(FlowError::Runtime)? != plan.digest
        {
            return Err(FlowError::Runtime(
                "WorkflowRun revision or payload authority changed".into(),
            ));
        }
        workflow::replay(
            invocation,
            &run,
            plan,
            &revision,
            &goal.goal.contract.spec().input,
        )
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        if invocation.step_name != types::WORKFLOW_LOCAL_STEP_NAME {
            return Err(FlowError::Runtime(format!(
                "Cloud WorkflowRun has no step {:?}",
                invocation.step_name
            )));
        }
        serde_json::to_value(local_step::execute(invocation.input_as()?)?).map_err(FlowError::from)
    }
}

fn repository_error(error: crate::modules::shared_kernel::domain::RepositoryError) -> FlowError {
    FlowError::Runtime(format!("WorkflowRun repository failed: {error}"))
}
