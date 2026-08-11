mod coordinator;
mod execution;
mod projection;
mod workflow;

pub use coordinator::FlowWorkflowRunCoordinator;
pub use projection::{project_workflow_run_record, WorkflowRunHistoryReader};

use crate::modules::shared_kernel::domain::Sha256Digest;
use crate::modules::workflow::domain::{
    ResolvedWorkflowRunStep, WorkflowRunInput, WorkflowStepKind,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION,
};
use a3s_flow::{FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const WORKFLOW_RUN_STEP_NAME: &str = "workflow_run_local";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkflowLocalStepInput {
    runtime_contract_revision: String,
    step: ResolvedWorkflowRunStep,
    workflow_input: serde_json::Value,
    effective_input: serde_json::Value,
    dependencies: BTreeMap<String, serde_json::Value>,
    steps: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowLocalStepResult {
    pub step_id: String,
    pub kind: WorkflowStepKind,
    pub output: serde_json::Value,
    pub output_digest: Sha256Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_handle: Option<String>,
}

impl WorkflowLocalStepResult {
    pub fn validate(&self, expected: &ResolvedWorkflowRunStep) -> Result<(), String> {
        if self.step_id != expected.plan.id || self.kind != expected.plan.kind {
            return Err("Workflow local step result identity drifted".into());
        }
        execution::validate_data_schema(
            &expected.output_schema,
            &self.output,
            "Workflow step output",
        )?;
        let digest = execution::value_digest(&self.output, "Workflow step output")?;
        if digest != self.output_digest {
            return Err("Workflow local step output digest does not match".into());
        }
        if self.kind == WorkflowStepKind::Branch {
            let selected = self
                .selected_handle
                .as_deref()
                .ok_or_else(|| "Workflow branch result did not select a handle".to_owned())?;
            if !expected
                .configuration
                .routes
                .iter()
                .any(|route| route.handle == selected)
            {
                return Err(format!(
                    "Workflow branch selected undeclared handle {selected:?}"
                ));
            }
        } else if self.selected_handle.is_some() {
            return Err("non-branch Workflow step selected a handle".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WorkflowRunFlowRuntime;

#[async_trait::async_trait]
impl FlowRuntime for WorkflowRunFlowRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> Result<RuntimeCommand, FlowError> {
        workflow::run_workflow(invocation)
    }

    async fn run_step(&self, invocation: StepInvocation) -> Result<serde_json::Value, FlowError> {
        if invocation.step_name != WORKFLOW_RUN_STEP_NAME {
            return Err(FlowError::Runtime(format!(
                "WorkflowRun runtime cannot execute step name {:?}",
                invocation.step_name
            )));
        }
        let input: WorkflowLocalStepInput = invocation.input_as()?;
        if input.runtime_contract_revision != WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION {
            return Err(FlowError::Runtime(
                "WorkflowRun step runtime contract revision is unsupported".into(),
            ));
        }
        let result = execution::execute_local_step(&input).map_err(FlowError::Runtime)?;
        serde_json::to_value(result).map_err(FlowError::from)
    }
}

fn decode_input(value: serde_json::Value) -> Result<WorkflowRunInput, FlowError> {
    let input: WorkflowRunInput = serde_json::from_value(value)?;
    input.validate().map_err(|error| {
        FlowError::InvalidWorkflow(format!("invalid WorkflowRun input: {error}"))
    })?;
    Ok(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{HumanTaskId, PrincipalId};
    use crate::modules::shared_kernel::domain::{Sha256Digest, WorkflowDecisionId};
    use crate::modules::workflow::domain::{
        flow_step_id, AssignmentPolicyRef, FlowResumePayload, HumanTask, NewHumanTask,
        WorkflowDecision, WorkflowRun, WorkflowRunRecord, WorkflowStepProjectionStatus,
        WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION,
    };
    use crate::modules::workflow::test_support::{
        accepted_submission, digest, human_decision_form_release,
        human_decision_workflow_run_input, timestamp, workflow_run_input, TEST_HUMAN_STEP_ID,
    };
    use a3s_flow::{
        FlowEngine, FlowEvent, HookStatus, RuntimeBuildCompatibility, RuntimeBuildId,
        WorkflowRunStatus, WorkflowSpec,
    };
    use serde_json::json;
    use std::sync::Arc;

    #[tokio::test]
    async fn flow_engine_executes_and_idempotently_replays_the_minimal_workflow_run(
    ) -> Result<(), FlowError> {
        let mut input = workflow_run_input().map_err(FlowError::Runtime)?;
        input.requested_at = chrono::Utc::now();
        input.deadline_at = input.requested_at + chrono::Duration::hours(1);
        input.validate().map_err(FlowError::Runtime)?;
        let run_id = input.workflow_run_id.to_string();
        let spec = WorkflowSpec::rust_embedded(
            WORKFLOW_RUN_FLOW_NAME,
            WORKFLOW_RUN_FLOW_VERSION,
            "cloud",
            "workflow_run",
        );
        let encoded = serde_json::to_value(&input)?;
        let engine = FlowEngine::in_memory(Arc::new(WorkflowRunFlowRuntime));
        engine
            .start_with_id(run_id.clone(), spec.clone(), encoded.clone())
            .await?;
        let snapshot = engine.snapshot(&run_id).await?;
        assert_eq!(
            snapshot.status,
            WorkflowRunStatus::Completed,
            "{snapshot:#?}"
        );
        assert_eq!(snapshot.output, Some(json!("HIGH T-42")));
        assert!(snapshot.steps.contains_key(&flow_step_id("high")));
        assert!(!snapshot.steps.contains_key(&flow_step_id("normal")));

        let history_length = engine.history(&run_id).await?.len();
        engine
            .start_with_id(run_id.clone(), spec.clone(), encoded)
            .await?;
        assert_eq!(engine.history(&run_id).await?.len(), history_length);

        let mut drifted = input;
        drifted.goal_input["ticketId"] = json!("T-99");
        assert!(engine
            .start_with_id(run_id, spec, serde_json::to_value(drifted)?)
            .await
            .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn flow_engine_suspends_human_decision_on_an_authority_bound_hook(
    ) -> Result<(), FlowError> {
        let mut input = human_decision_workflow_run_input().map_err(FlowError::Runtime)?;
        input.requested_at = chrono::Utc::now();
        input.deadline_at = input.requested_at + chrono::Duration::hours(1);
        input.validate().map_err(FlowError::Runtime)?;
        let run_id = input.workflow_run_id.to_string();
        let (run, steps) =
            WorkflowRun::create(input.clone(), PrincipalId::new()).map_err(FlowError::Runtime)?;
        let record = WorkflowRunRecord { run, steps };
        let runtime_build_id = RuntimeBuildId::new("a3s-cloud-human-decision-test@1")?;
        let spec = WorkflowSpec::rust_embedded(
            WORKFLOW_RUN_FLOW_NAME,
            WORKFLOW_RUN_FLOW_VERSION,
            "a3s-cloud",
            "main",
        )
        .with_runtime_build(runtime_build_id.clone());
        let engine = FlowEngine::builder(Arc::new(WorkflowRunFlowRuntime))
            .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(runtime_build_id))
            .build();
        engine
            .start_with_id(run_id.clone(), spec, serde_json::to_value(&input)?)
            .await?;

        let snapshot = engine.snapshot(&run_id).await?;
        assert_eq!(snapshot.status, WorkflowRunStatus::Suspended);
        assert!(!snapshot
            .steps
            .contains_key(&flow_step_id(TEST_HUMAN_STEP_ID)));
        let hook_id = format!("workflow-human:{TEST_HUMAN_STEP_ID}:1");
        let hook = snapshot.hooks.get(&hook_id).expect("human-decision hook");
        assert_eq!(hook.status, HookStatus::Active);
        assert_eq!(
            hook.token,
            format!("workflow-human:{run_id}:{TEST_HUMAN_STEP_ID}:1")
        );
        let capability = input.plan.steps[1]
            .capability
            .as_ref()
            .expect("FormRelease capability");
        assert_eq!(
            hook.metadata,
            json!({
                "schema": "cloud.workflow.human-decision-hook.v1",
                "organizationId": input.organization_id,
                "projectId": input.project_id,
                "workflowRunId": input.workflow_run_id,
                "planRevisionId": input.plan_revision_id,
                "planDigest": input.plan_digest,
                "stepId": TEST_HUMAN_STEP_ID,
                "stepAttempt": 1,
                "configurationDigest": input.plan.steps[1].configuration_digest,
                "formId": capability.resource_id,
                "formReleaseId": capability.revision,
                "formReleaseDigest": capability.digest,
            })
        );
        let history = engine.history(&run_id).await?;
        assert!(history.iter().any(|event| {
            matches!(
                &event.event,
                FlowEvent::HookCreated {
                    hook_id: observed,
                    metadata,
                    ..
                } if observed == &hook_id && metadata == &hook.metadata
            )
        }));
        let mut drifted_snapshot = snapshot.clone();
        drifted_snapshot
            .hooks
            .get_mut(&hook_id)
            .expect("drifted human-decision hook")
            .metadata["formReleaseDigest"] = json!(digest('f'));
        assert!(project_workflow_run_record(&record, &drifted_snapshot, &history).is_err());
        let mut drifted_token_snapshot = snapshot.clone();
        drifted_token_snapshot
            .hooks
            .get_mut(&hook_id)
            .expect("drifted human-decision hook")
            .token = "drifted-human-hook-token".into();
        assert!(project_workflow_run_record(&record, &drifted_token_snapshot, &history).is_err());
        let waiting_record = project_workflow_run_record(&record, &snapshot, &history)
            .map_err(FlowError::Runtime)?
            .expect("waiting WorkflowRun projection");
        assert_eq!(
            waiting_record.run.status,
            crate::modules::workflow::domain::WorkflowRunStatus::Waiting
        );
        let waiting_step = waiting_record
            .steps
            .iter()
            .find(|step| step.step_id == TEST_HUMAN_STEP_ID)
            .expect("waiting human-decision projection");
        assert_eq!(waiting_step.status, WorkflowStepProjectionStatus::Running);
        assert_eq!(waiting_step.attempt_generation, 1);

        let principal_id = PrincipalId::new();
        let mut task = HumanTask::create(NewHumanTask {
            organization_id: input.organization_id,
            project_id: input.project_id,
            id: HumanTaskId::new(),
            workflow_run_id: input.workflow_run_id,
            step_id: TEST_HUMAN_STEP_ID.into(),
            step_attempt: 1,
            form_release: human_decision_form_release(&input).map_err(FlowError::Runtime)?,
            assignment_policy: AssignmentPolicyRef::new(
                "approval-policy",
                1,
                Sha256Digest::parse(digest('b')).map_err(FlowError::Runtime)?,
            )
            .map_err(FlowError::Runtime)?,
            flow_run_id: run_id.clone(),
            flow_hook_id: hook_id.clone(),
            due_at: Some(timestamp(9, 0)),
            expires_at: Some(timestamp(10, 0)),
            created_at: timestamp(8, 0),
        })
        .map_err(FlowError::Runtime)?;
        task.activate(1, timestamp(8, 1))
            .map_err(FlowError::Runtime)?;
        task.claim(2, principal_id, timestamp(8, 2))
            .map_err(FlowError::Runtime)?;
        let submission = accepted_submission(&task, principal_id);
        let decision = WorkflowDecision::from_submission(
            WorkflowDecisionId::new(),
            &task,
            &submission,
            submission.accepted_output().map_err(FlowError::Runtime)?,
            timestamp(8, 31),
        )
        .map_err(FlowError::Runtime)?;
        let resume_payload =
            FlowResumePayload::from_decision(&decision).map_err(FlowError::Runtime)?;
        engine
            .resume_hook(
                &run_id,
                &hook_id,
                resume_payload.to_flow_value().map_err(FlowError::Runtime)?,
            )
            .await?;
        let completed = engine.snapshot(&run_id).await?;
        assert_eq!(completed.status, WorkflowRunStatus::Completed);
        assert_eq!(
            completed.output,
            Some(serde_json::to_value(&resume_payload.output)?)
        );
        assert_eq!(completed.hooks[&hook_id].status, HookStatus::Received);
        assert!(!completed
            .steps
            .contains_key(&flow_step_id(TEST_HUMAN_STEP_ID)));
        let completed_history = engine.history(&run_id).await?;
        let completed_record =
            project_workflow_run_record(&waiting_record, &completed, &completed_history)
                .map_err(FlowError::Runtime)?
                .expect("completed WorkflowRun projection");
        assert_eq!(
            completed_record.run.status,
            crate::modules::workflow::domain::WorkflowRunStatus::Completed
        );
        let completed_step = completed_record
            .steps
            .iter()
            .find(|step| step.step_id == TEST_HUMAN_STEP_ID)
            .expect("completed human-decision projection");
        assert_eq!(
            completed_step.status,
            WorkflowStepProjectionStatus::Completed
        );
        assert_eq!(
            completed_step.result,
            Some(serde_json::to_value(&resume_payload.output)?)
        );
        Ok(())
    }
}
