mod composite;
mod coordinator;
mod execution;
mod projection;
mod readers;
mod variables;
mod workflow;

pub use coordinator::FlowWorkflowRunCoordinator;
pub use projection::project_workflow_run_record;
pub use readers::{WorkflowRunHistoryReader, WorkflowRunVariableReader};

use crate::modules::shared_kernel::domain::Sha256Digest;
use crate::modules::workflow::domain::{
    ResolvedWorkflowRunStep, WorkflowRunInput, WorkflowStepKind,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V3,
};
use a3s_flow::{FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const WORKFLOW_RUN_STEP_NAME: &str = "workflow_run_local";

pub(crate) fn flow_step_names() -> impl Iterator<Item = &'static str> {
    std::iter::once(WORKFLOW_RUN_STEP_NAME)
}

pub(crate) fn flow_workflow_identities() -> impl Iterator<Item = (&'static str, &'static str)> {
    [
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V2,
        ),
        (
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_NAME,
            crate::modules::workflow::domain::WORKFLOW_RUN_FLOW_VERSION_V3,
        ),
    ]
    .into_iter()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkflowLocalStepInput {
    runtime_contract_revision: String,
    #[serde(default, skip_serializing_if = "is_false")]
    typed_projection_authoritative: bool,
    step: ResolvedWorkflowRunStep,
    workflow_input: serde_json::Value,
    effective_input: serde_json::Value,
    dependencies: BTreeMap<String, serde_json::Value>,
    steps: BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    composite_region_result:
        Option<crate::modules::workflow::domain::WorkflowCompositeRegionResult>,
}

const fn is_false(value: &bool) -> bool {
    !*value
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composite_region_result:
        Option<crate::modules::workflow::domain::WorkflowCompositeRegionResult>,
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
        match (self.kind, self.composite_region_result.as_ref()) {
            (WorkflowStepKind::Subworkflow, Some(region))
                if region.region_step_id == self.step_id
                    && region.output == self.output
                    && region.output_digest == self.output_digest => {}
            (WorkflowStepKind::Subworkflow, _) => {
                return Err("Workflow composite step result lost its region evidence".into())
            }
            (_, None) => {}
            (_, Some(_)) => {
                return Err("non-composite Workflow step retained region evidence".into())
            }
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
        if !matches!(
            input.runtime_contract_revision.as_str(),
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION
                | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2
                | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V3
        ) {
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
        flow_step_id, AssignmentPolicyRef, FlowResumePayload, HumanTask,
        IWorkflowRunVariableReader, NewHumanTask, WorkflowCompositeFrameResolution,
        WorkflowCompositeHookMetadata, WorkflowCompositeRegionPolicy,
        WorkflowCompositeResumePayload, WorkflowDecision, WorkflowIterationFailureMode,
        WorkflowIterationRegionPolicy, WorkflowLoopRegionPolicy, WorkflowRun, WorkflowRunRecord,
        WorkflowRunVariableState, WorkflowStepProjectionStatus, WORKFLOW_RUN_FLOW_NAME,
        WORKFLOW_RUN_FLOW_VERSION, WORKFLOW_RUN_FLOW_VERSION_V2, WORKFLOW_RUN_FLOW_VERSION_V3,
    };
    use crate::modules::workflow::test_support::{
        accepted_submission, composite_workflow_run_input, digest,
        exclusive_output_workflow_run_input, human_decision_form_release,
        human_decision_workflow_run_input, multi_output_workflow_run_input, timestamp,
        typed_variable_workflow_run_input, workflow_run_input, TEST_HUMAN_STEP_ID,
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
    async fn flow_engine_executes_and_idempotently_replays_plan_v2_variable_reads(
    ) -> Result<(), FlowError> {
        let mut input = typed_variable_workflow_run_input().map_err(FlowError::Runtime)?;
        input.requested_at = chrono::Utc::now();
        input.deadline_at = input.requested_at + chrono::Duration::hours(1);
        input.validate().map_err(FlowError::Runtime)?;
        let run_id = input.workflow_run_id.to_string();
        let spec = WorkflowSpec::rust_embedded(
            WORKFLOW_RUN_FLOW_NAME,
            WORKFLOW_RUN_FLOW_VERSION_V2,
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
        assert_eq!(snapshot.output, Some(json!({"result": input.goal_input})));
        let history_length = engine.history(&run_id).await?.len();
        engine.start_with_id(&run_id, spec, encoded).await?;
        assert_eq!(engine.history(&run_id).await?.len(), history_length);
        Ok(())
    }

    #[tokio::test]
    async fn variable_reader_reconstructs_values_from_the_same_flow_history(
    ) -> Result<(), FlowError> {
        let mut input = typed_variable_workflow_run_input().map_err(FlowError::Runtime)?;
        input.requested_at = chrono::Utc::now();
        input.deadline_at = input.requested_at + chrono::Duration::hours(1);
        input.validate().map_err(FlowError::Runtime)?;
        let (run, steps) =
            WorkflowRun::create(input.clone(), PrincipalId::new()).map_err(FlowError::Runtime)?;
        let record = WorkflowRunRecord { run, steps };
        let run_id = input.workflow_run_id.to_string();
        let engine = FlowEngine::in_memory(Arc::new(WorkflowRunFlowRuntime));
        engine
            .start_with_id(
                &run_id,
                WorkflowSpec::rust_embedded(
                    WORKFLOW_RUN_FLOW_NAME,
                    WORKFLOW_RUN_FLOW_VERSION_V2,
                    "a3s-cloud",
                    "main",
                ),
                serde_json::to_value(&input)?,
            )
            .await?;

        let snapshot = engine.snapshot(&run_id).await?;
        let inspection = WorkflowRunVariableReader::new(engine)
            .inspect(&record)
            .await
            .map_err(FlowError::Runtime)?;

        assert_eq!(inspection.last_flow_sequence, snapshot.last_sequence);
        assert_eq!(inspection.variables.len(), 1);
        assert_eq!(
            inspection.variables[0].state,
            WorkflowRunVariableState::Materialized
        );
        assert_eq!(
            inspection.variables[0].value.as_ref(),
            Some(&input.goal_input)
        );
        Ok(())
    }

    #[tokio::test]
    async fn flow_engine_waits_for_and_deterministically_aggregates_reachable_output_sinks(
    ) -> Result<(), FlowError> {
        let mut input = multi_output_workflow_run_input().map_err(FlowError::Runtime)?;
        input.requested_at = chrono::Utc::now();
        input.deadline_at = input.requested_at + chrono::Duration::hours(1);
        input.validate().map_err(FlowError::Runtime)?;
        let run_id = input.workflow_run_id.to_string();
        let engine = FlowEngine::in_memory(Arc::new(WorkflowRunFlowRuntime));
        engine
            .start_with_id(
                &run_id,
                WorkflowSpec::rust_embedded(
                    WORKFLOW_RUN_FLOW_NAME,
                    WORKFLOW_RUN_FLOW_VERSION,
                    "cloud",
                    "workflow_run",
                ),
                serde_json::to_value(&input)?,
            )
            .await?;

        let snapshot = engine.snapshot(&run_id).await?;
        assert_eq!(
            snapshot.status,
            WorkflowRunStatus::Completed,
            "{snapshot:#?}"
        );
        assert_eq!(
            snapshot.output,
            Some(json!({
                "output-a": "HIGH T-42",
                "output-b": "HIGH T-42",
            }))
        );
        assert!(snapshot.steps.contains_key(&flow_step_id("output-a")));
        assert!(snapshot.steps.contains_key(&flow_step_id("output-b")));
        assert!(!snapshot.steps.contains_key(&flow_step_id("normal")));
        Ok(())
    }

    #[tokio::test]
    async fn flow_engine_omits_inactive_output_sinks_from_the_terminal_aggregate(
    ) -> Result<(), FlowError> {
        let mut input = exclusive_output_workflow_run_input().map_err(FlowError::Runtime)?;
        input.requested_at = chrono::Utc::now();
        input.deadline_at = input.requested_at + chrono::Duration::hours(1);
        input.validate().map_err(FlowError::Runtime)?;
        let run_id = input.workflow_run_id.to_string();
        let engine = FlowEngine::in_memory(Arc::new(WorkflowRunFlowRuntime));
        engine
            .start_with_id(
                &run_id,
                WorkflowSpec::rust_embedded(
                    WORKFLOW_RUN_FLOW_NAME,
                    WORKFLOW_RUN_FLOW_VERSION,
                    "cloud",
                    "workflow_run",
                ),
                serde_json::to_value(&input)?,
            )
            .await?;

        let snapshot = engine.snapshot(&run_id).await?;
        assert_eq!(
            snapshot.status,
            WorkflowRunStatus::Completed,
            "{snapshot:#?}"
        );
        assert_eq!(snapshot.output, Some(json!({"output-a": "HIGH T-42"})));
        assert!(snapshot.steps.contains_key(&flow_step_id("output-a")));
        assert!(!snapshot.steps.contains_key(&flow_step_id("output-b")));
        assert!(!snapshot.steps.contains_key(&flow_step_id("normal")));
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

    #[tokio::test]
    async fn expiry_resume_winning_the_race_commits_hook_evidence_before_typed_timeout(
    ) -> Result<(), FlowError> {
        let mut input = human_decision_workflow_run_input().map_err(FlowError::Runtime)?;
        input.requested_at = chrono::Utc::now();
        input.deadline_at = input.requested_at + chrono::Duration::seconds(1);
        input.validate().map_err(FlowError::Runtime)?;
        let run_id = input.workflow_run_id.to_string();
        let engine = FlowEngine::in_memory(Arc::new(WorkflowRunFlowRuntime));
        engine
            .start_with_id(
                &run_id,
                WorkflowSpec::rust_embedded(
                    WORKFLOW_RUN_FLOW_NAME,
                    WORKFLOW_RUN_FLOW_VERSION,
                    "cloud",
                    "workflow_run",
                ),
                serde_json::to_value(&input)?,
            )
            .await?;
        let history = engine.history(&run_id).await?;
        let hook_id = format!("workflow-human:{TEST_HUMAN_STEP_ID}:1");
        let hook_created = history
            .iter()
            .find(|event| {
                matches!(
                    &event.event,
                    FlowEvent::HookCreated { hook_id: observed, .. } if observed == &hook_id
                )
            })
            .expect("human hook");
        let mut task = HumanTask::create(NewHumanTask {
            organization_id: input.organization_id,
            project_id: input.project_id,
            id: HumanTaskId::new(),
            workflow_run_id: input.workflow_run_id,
            step_id: TEST_HUMAN_STEP_ID.into(),
            step_attempt: 1,
            form_release: human_decision_form_release(&input).map_err(FlowError::Runtime)?,
            assignment_policy: AssignmentPolicyRef::workflow_organization_member_exclusive()
                .map_err(FlowError::Runtime)?,
            flow_run_id: run_id.clone(),
            flow_hook_id: hook_id.clone(),
            due_at: None,
            expires_at: Some(input.deadline_at),
            created_at: hook_created.timestamp,
        })
        .map_err(FlowError::Runtime)?;
        task.activate(1, hook_created.timestamp)
            .map_err(FlowError::Runtime)?;
        let decision = WorkflowDecision::expire(
            WorkflowDecisionId::new(),
            &task,
            PrincipalId::new(),
            crate::modules::shared_kernel::domain::AuthorizationDecisionRef::new(
                "deadline-authority",
                Sha256Digest::parse(digest('d')).map_err(FlowError::Runtime)?,
            )
            .map_err(FlowError::Runtime)?,
            input.deadline_at,
        )
        .map_err(FlowError::Runtime)?;
        let payload = FlowResumePayload::from_decision(&decision).map_err(FlowError::Runtime)?;

        let remaining = (input.deadline_at - chrono::Utc::now())
            .to_std()
            .unwrap_or_default()
            .saturating_add(std::time::Duration::from_millis(5));
        tokio::time::sleep(remaining).await;
        engine
            .resume_hook(
                &run_id,
                &hook_id,
                payload.to_flow_value().map_err(FlowError::Runtime)?,
            )
            .await?;

        let history = engine.history(&run_id).await?;
        let hook_sequence = history
            .iter()
            .find_map(|event| {
                matches!(
                    &event.event,
                    FlowEvent::HookReceived { hook_id: observed, .. } if observed == &hook_id
                )
                .then_some(event.sequence)
            })
            .expect("HookReceived evidence");
        let timeout_sequence = history
            .iter()
            .find_map(|event| {
                matches!(
                    &event.event,
                    FlowEvent::RunTimedOut { deadline, .. } if deadline == &input.deadline_at
                )
                .then_some(event.sequence)
            })
            .expect("RunTimedOut evidence");
        assert!(hook_sequence < timeout_sequence);
        let snapshot = engine.snapshot(&run_id).await?;
        assert_eq!(snapshot.status, WorkflowRunStatus::Failed);
        assert!(matches!(
            snapshot.terminal_outcome,
            Some(a3s_flow::WorkflowTerminalOutcome::TimedOut { deadline, .. })
                if deadline == input.deadline_at
        ));
        Ok(())
    }

    #[tokio::test]
    async fn runtime_v3_dispatches_and_reduces_iteration_frames_in_ordinal_order(
    ) -> Result<(), FlowError> {
        let mut input = composite_workflow_run_input(
            WorkflowCompositeRegionPolicy::Iteration(WorkflowIterationRegionPolicy {
                step_id: "batch".into(),
                maximum_items: 3,
                maximum_concurrency: 1,
                failure_mode: WorkflowIterationFailureMode::Terminate,
            }),
            json!([{"item": 1}, {"item": 2}]),
        )
        .map_err(FlowError::Runtime)?;
        input.requested_at = chrono::Utc::now();
        input.deadline_at = input.requested_at + chrono::Duration::hours(1);
        input.validate().map_err(FlowError::Runtime)?;
        let run_id = input.workflow_run_id.to_string();
        let engine = FlowEngine::in_memory(Arc::new(WorkflowRunFlowRuntime));
        engine
            .start_with_id(
                &run_id,
                WorkflowSpec::rust_embedded(
                    WORKFLOW_RUN_FLOW_NAME,
                    WORKFLOW_RUN_FLOW_VERSION_V3,
                    "a3s-cloud",
                    "main",
                ),
                serde_json::to_value(&input)?,
            )
            .await?;

        let first = composite_hook(&engine, &run_id, "batch", 0).await?;
        assert_eq!(first.frame.child_input, json!({"item": 1}));
        resume_completed_composite(&engine, &run_id, &input, first, json!({"value": 10})).await?;
        let second = composite_hook(&engine, &run_id, "batch", 1).await?;
        assert_eq!(second.frame.child_input, json!({"item": 2}));
        resume_completed_composite(&engine, &run_id, &input, second, json!({"value": 20})).await?;

        let snapshot = engine.snapshot(&run_id).await?;
        assert_eq!(
            snapshot.status,
            WorkflowRunStatus::Completed,
            "{snapshot:#?}"
        );
        assert_eq!(snapshot.output, Some(json!([{"value": 10}, {"value": 20}])));
        let result = snapshot.steps[&flow_step_id("batch")]
            .output
            .clone()
            .ok_or_else(|| FlowError::Runtime("composite finalizer has no output".into()))?;
        let result = serde_json::from_value::<WorkflowLocalStepResult>(result)?;
        let region = result
            .composite_region_result
            .ok_or_else(|| FlowError::Runtime("composite finalizer lost its evidence".into()))?;
        assert_eq!(region.frames.len(), 2);
        assert_eq!(region.frames[0].ordinal(), 0);
        assert_eq!(region.frames[1].ordinal(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn runtime_v3_loop_feeds_terminal_child_output_into_the_next_frame(
    ) -> Result<(), FlowError> {
        let mut input = composite_workflow_run_input(
            WorkflowCompositeRegionPolicy::Loop(WorkflowLoopRegionPolicy {
                step_id: "refine".into(),
                maximum_iterations: 3,
                time_budget_seconds: 3_600,
                termination_path: vec!["done".into()],
            }),
            json!({"iteration": 0}),
        )
        .map_err(FlowError::Runtime)?;
        input.requested_at = chrono::Utc::now();
        input.deadline_at = input.requested_at + chrono::Duration::hours(1);
        input.validate().map_err(FlowError::Runtime)?;
        let run_id = input.workflow_run_id.to_string();
        let engine = FlowEngine::in_memory(Arc::new(WorkflowRunFlowRuntime));
        engine
            .start_with_id(
                &run_id,
                WorkflowSpec::rust_embedded(
                    WORKFLOW_RUN_FLOW_NAME,
                    WORKFLOW_RUN_FLOW_VERSION_V3,
                    "a3s-cloud",
                    "main",
                ),
                serde_json::to_value(&input)?,
            )
            .await?;

        let first = composite_hook(&engine, &run_id, "refine", 0).await?;
        resume_completed_composite(
            &engine,
            &run_id,
            &input,
            first,
            json!({"done": false, "iteration": 1}),
        )
        .await?;
        let second = composite_hook(&engine, &run_id, "refine", 1).await?;
        assert_eq!(
            second.frame.child_input,
            json!({"done": false, "iteration": 1})
        );
        resume_completed_composite(
            &engine,
            &run_id,
            &input,
            second,
            json!({"done": true, "iteration": 2}),
        )
        .await?;

        let snapshot = engine.snapshot(&run_id).await?;
        assert_eq!(
            snapshot.status,
            WorkflowRunStatus::Completed,
            "{snapshot:#?}"
        );
        assert_eq!(snapshot.output, Some(json!({"done": true, "iteration": 2})));
        Ok(())
    }

    #[tokio::test]
    async fn runtime_v3_distinguishes_valid_child_failure_from_resume_authority_drift(
    ) -> Result<(), FlowError> {
        let policy = WorkflowCompositeRegionPolicy::Iteration(WorkflowIterationRegionPolicy {
            step_id: "batch".into(),
            maximum_items: 1,
            maximum_concurrency: 1,
            failure_mode: WorkflowIterationFailureMode::Terminate,
        });
        let mut input = composite_workflow_run_input(policy.clone(), json!([{"item": 1}]))
            .map_err(FlowError::Runtime)?;
        input.requested_at = chrono::Utc::now();
        input.deadline_at = input.requested_at + chrono::Duration::hours(1);
        input.validate().map_err(FlowError::Runtime)?;
        let run_id = input.workflow_run_id.to_string();
        let engine = FlowEngine::in_memory(Arc::new(WorkflowRunFlowRuntime));
        let spec = WorkflowSpec::rust_embedded(
            WORKFLOW_RUN_FLOW_NAME,
            WORKFLOW_RUN_FLOW_VERSION_V3,
            "a3s-cloud",
            "main",
        );
        engine
            .start_with_id(&run_id, spec.clone(), serde_json::to_value(&input)?)
            .await?;
        let metadata = composite_hook(&engine, &run_id, "batch", 0).await?;
        let variables = input
            .variable_contract
            .as_ref()
            .ok_or_else(|| FlowError::Runtime("missing variable contract".into()))?
            .restore()
            .map_err(FlowError::Runtime)?;
        let regions = input
            .composite_regions
            .as_ref()
            .ok_or_else(|| FlowError::Runtime("missing composite regions".into()))?
            .restore()
            .map_err(FlowError::Runtime)?;
        let failure = WorkflowCompositeResumePayload::new(
            &metadata,
            WorkflowCompositeFrameResolution::failed(
                metadata.frame.clone(),
                "child WorkflowRun failed",
            ),
            &input.plan,
            &regions,
            &variables,
        )
        .map_err(FlowError::Runtime)?;
        engine
            .resume_hook(
                &run_id,
                &metadata.flow_hook_id(),
                serde_json::to_value(failure)?,
            )
            .await?;
        let failed = engine.snapshot(&run_id).await?;
        assert_eq!(failed.status, WorkflowRunStatus::Failed);
        assert!(failed
            .error
            .as_deref()
            .is_some_and(|error| error.contains("child WorkflowRun failed")));

        let mut drifted_input = composite_workflow_run_input(policy, json!([{"item": 1}]))
            .map_err(FlowError::Runtime)?;
        drifted_input.requested_at = chrono::Utc::now();
        drifted_input.deadline_at = drifted_input.requested_at + chrono::Duration::hours(1);
        drifted_input.validate().map_err(FlowError::Runtime)?;
        let drifted_run_id = drifted_input.workflow_run_id.to_string();
        engine
            .start_with_id(&drifted_run_id, spec, serde_json::to_value(&drifted_input)?)
            .await?;
        let drifted_metadata = composite_hook(&engine, &drifted_run_id, "batch", 0).await?;
        let drifted_variables = drifted_input
            .variable_contract
            .as_ref()
            .ok_or_else(|| FlowError::Runtime("missing variable contract".into()))?
            .restore()
            .map_err(FlowError::Runtime)?;
        let drifted_regions = drifted_input
            .composite_regions
            .as_ref()
            .ok_or_else(|| FlowError::Runtime("missing composite regions".into()))?
            .restore()
            .map_err(FlowError::Runtime)?;
        let result = drifted_metadata
            .frame
            .resolve(
                &drifted_input.plan,
                &drifted_regions,
                &drifted_variables,
                json!({"value": 10}),
            )
            .map_err(FlowError::Runtime)?;
        let payload = WorkflowCompositeResumePayload::new(
            &drifted_metadata,
            WorkflowCompositeFrameResolution::completed(drifted_metadata.frame.clone(), result),
            &drifted_input.plan,
            &drifted_regions,
            &drifted_variables,
        )
        .map_err(FlowError::Runtime)?;
        let mut payload = serde_json::to_value(payload)?;
        payload["payloadDigest"] = json!(digest('f'));
        let error = engine
            .resume_hook(&drifted_run_id, &drifted_metadata.flow_hook_id(), payload)
            .await
            .expect_err("tampered composite resume must fail closed");
        assert!(matches!(error, FlowError::NonDeterministic { .. }));
        Ok(())
    }

    async fn composite_hook(
        engine: &FlowEngine,
        run_id: &str,
        step_id: &str,
        ordinal: u32,
    ) -> Result<WorkflowCompositeHookMetadata, FlowError> {
        let hook_id = format!("workflow-composite:{step_id}:{ordinal}");
        let snapshot = engine.snapshot(run_id).await?;
        let hook = snapshot
            .hooks
            .get(&hook_id)
            .ok_or_else(|| FlowError::Runtime(format!("missing composite hook {hook_id}")))?;
        assert_eq!(hook.status, HookStatus::Active, "{snapshot:#?}");
        serde_json::from_value(hook.metadata.clone()).map_err(FlowError::Serialization)
    }

    async fn resume_completed_composite(
        engine: &FlowEngine,
        run_id: &str,
        input: &crate::modules::workflow::domain::WorkflowRunInput,
        metadata: WorkflowCompositeHookMetadata,
        output: serde_json::Value,
    ) -> Result<(), FlowError> {
        let variables = input
            .variable_contract
            .as_ref()
            .ok_or_else(|| FlowError::Runtime("missing variable contract".into()))?
            .restore()
            .map_err(FlowError::Runtime)?;
        let regions = input
            .composite_regions
            .as_ref()
            .ok_or_else(|| FlowError::Runtime("missing composite regions".into()))?
            .restore()
            .map_err(FlowError::Runtime)?;
        let result = metadata
            .frame
            .resolve(&input.plan, &regions, &variables, output)
            .map_err(FlowError::Runtime)?;
        let resolution =
            WorkflowCompositeFrameResolution::completed(metadata.frame.clone(), result);
        let payload = WorkflowCompositeResumePayload::new(
            &metadata,
            resolution,
            &input.plan,
            &regions,
            &variables,
        )
        .map_err(FlowError::Runtime)?;
        engine
            .resume_hook(
                run_id,
                &metadata.flow_hook_id(),
                serde_json::to_value(payload)?,
            )
            .await
    }
}
