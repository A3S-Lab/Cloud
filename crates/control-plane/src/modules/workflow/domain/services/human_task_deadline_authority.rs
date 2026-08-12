use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, AuthorizationDecisionRef,
    HumanTaskId, OrganizationId, PlanRevisionId, PrincipalId, ProjectId, Sha256Digest,
    WorkflowRunId,
};
use crate::modules::workflow::domain::{
    AssignmentPolicyRef, HumanTaskRecord, HumanTaskStatus, ResolvedWorkflowRunStep,
    WorkflowHumanDecisionHookMetadata, WorkflowRunRecord, WorkflowRunStatus, WorkflowStepKind,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

pub const HUMAN_TASK_DEADLINE_AUTHORITY_API_VERSION: &str =
    "a3s.dev/workflow/human-task-deadline-authority/v1";
pub const HUMAN_TASK_CANCELLATION_AUTHORITY_API_VERSION: &str =
    "a3s.dev/workflow/human-task-parent-cancellation-authority/v1";
const HUMAN_TASK_DEADLINE_EVIDENCE_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanTaskDeadlineAuthority {
    pub decided_by: PrincipalId,
    pub authorization_decision: AuthorizationDecisionRef,
    pub decided_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanTaskCancellationAuthority {
    pub decided_by: PrincipalId,
    pub authorization_decision: AuthorizationDecisionRef,
    pub decided_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanTaskParentCancellationEvidence {
    pub flow_run_id: String,
    pub request_sequence: u64,
    pub request_event_id: Uuid,
    pub request_event_at: DateTime<Utc>,
    pub request_reason: Option<String>,
    pub cancelled_sequence: u64,
    pub cancelled_event_id: Uuid,
    pub cancelled_event_at: DateTime<Utc>,
    pub cancelled_reason: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HumanTaskDeadlineEvidence<'a> {
    api_version: &'static str,
    organization_id: OrganizationId,
    project_id: ProjectId,
    workflow_run_id: WorkflowRunId,
    plan_revision_id: PlanRevisionId,
    plan_digest: &'a Sha256Digest,
    execution_input_digest: &'a Sha256Digest,
    workflow_deadline_at: DateTime<Utc>,
    requested_by: PrincipalId,
    human_task_id: HumanTaskId,
    task_version: u64,
    task_status: HumanTaskStatus,
    task_updated_at: DateTime<Utc>,
    step_id: &'a str,
    step_attempt: u64,
    configuration_digest: &'a Sha256Digest,
    flow_run_id: &'a str,
    flow_hook_id: &'a str,
    hook_event_sequence: u64,
    hook_event_id: uuid::Uuid,
    task_created_at: DateTime<Utc>,
    task_expires_at: DateTime<Utc>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HumanTaskCancellationAuthorityEvidence<'a> {
    api_version: &'static str,
    organization_id: OrganizationId,
    project_id: ProjectId,
    workflow_run_id: WorkflowRunId,
    plan_revision_id: PlanRevisionId,
    plan_digest: &'a Sha256Digest,
    execution_input_digest: &'a Sha256Digest,
    cancellation_requested_by: PrincipalId,
    cancellation_requested_at: DateTime<Utc>,
    cancellation_reason: &'a Option<String>,
    human_task_id: HumanTaskId,
    task_version: u64,
    task_status: HumanTaskStatus,
    task_updated_at: DateTime<Utc>,
    step_id: &'a str,
    step_attempt: u64,
    configuration_digest: &'a Sha256Digest,
    flow_run_id: &'a str,
    flow_hook_id: &'a str,
    hook_event_sequence: u64,
    hook_event_id: Uuid,
    request_sequence: u64,
    request_event_id: Uuid,
    request_event_at: DateTime<Utc>,
    cancelled_sequence: u64,
    cancelled_event_id: Uuid,
    cancelled_event_at: DateTime<Utc>,
}

impl HumanTaskDeadlineAuthority {
    pub fn derive(run: &WorkflowRunRecord, task: &HumanTaskRecord) -> Result<Self, String> {
        let (step, hook) = exact_human_task_authority(run, task)?;

        let expires_at = expected_human_task_expiry(run, &step, task.task.created_at)?;
        if task.task.expires_at != Some(expires_at) {
            return Err("HumanTask expiry drifted from its immutable WorkflowRun authority".into());
        }

        let evidence = HumanTaskDeadlineEvidence {
            api_version: HUMAN_TASK_DEADLINE_AUTHORITY_API_VERSION,
            organization_id: task.task.organization_id,
            project_id: task.task.project_id,
            workflow_run_id: task.task.workflow_run_id,
            plan_revision_id: run.run.plan_revision_id,
            plan_digest: &run.run.plan_digest,
            execution_input_digest: &run.run.execution_input_digest,
            workflow_deadline_at: run.run.execution_input.deadline_at,
            requested_by: run.run.requested_by,
            human_task_id: task.task.id,
            task_version: task.task.aggregate_version,
            task_status: task.task.status,
            task_updated_at: task.task.updated_at,
            step_id: &task.task.step_id,
            step_attempt: task.task.step_attempt,
            configuration_digest: &hook.configuration_digest,
            flow_run_id: &task.task.flow_run_id,
            flow_hook_id: &task.task.flow_hook_id,
            hook_event_sequence: task.hook_event_sequence,
            hook_event_id: task.hook_event_id,
            task_created_at: task.task.created_at,
            task_expires_at: expires_at,
        };
        let canonical = canonical_json_bounded(
            &evidence,
            HUMAN_TASK_DEADLINE_EVIDENCE_MAX_BYTES,
            "HumanTask deadline authority evidence",
        )?;
        let digest = Sha256Digest::parse(sha256_digest(&canonical))?;
        let authorization_decision = AuthorizationDecisionRef::new(
            format!(
                "urn:a3s:cloud:workflow:human-task-deadline:{}:v{}",
                task.task.id, task.task.aggregate_version
            ),
            digest,
        )?;

        Ok(Self {
            decided_by: run.run.requested_by,
            authorization_decision,
            decided_at: expires_at,
        })
    }
}

impl HumanTaskCancellationAuthority {
    pub fn derive(
        run: &WorkflowRunRecord,
        task: &HumanTaskRecord,
        cancellation: &HumanTaskParentCancellationEvidence,
    ) -> Result<Self, String> {
        let (_, hook) = exact_human_task_authority(run, task)?;
        let requested_by = run.run.cancellation_requested_by.ok_or_else(|| {
            "WorkflowRun cancellation authority has no requesting principal".to_owned()
        })?;
        let requested_at = run
            .run
            .cancellation_requested_at
            .ok_or_else(|| "WorkflowRun cancellation authority has no request time".to_owned())?;
        if run.run.status != WorkflowRunStatus::Cancelled
            || cancellation.flow_run_id != run.run.flow_run_id
            || cancellation.request_event_id.is_nil()
            || cancellation.cancelled_event_id.is_nil()
        {
            return Err("HumanTask parent cancellation identity is invalid".into());
        }
        if cancellation.request_sequence <= task.hook_event_sequence
            || cancellation.cancelled_sequence <= cancellation.request_sequence
        {
            return Err(format!(
                "HumanTask parent cancellation sequence is invalid: hook={}, request={}, cancelled={}",
                task.hook_event_sequence,
                cancellation.request_sequence,
                cancellation.cancelled_sequence
            ));
        }
        if cancellation.request_event_at != canonical_timestamp(cancellation.request_event_at)
            || cancellation.cancelled_event_at
                != canonical_timestamp(cancellation.cancelled_event_at)
            || cancellation.request_event_at < requested_at
            || cancellation.request_event_at < task.task.created_at
            || cancellation.cancelled_event_at < cancellation.request_event_at
        {
            return Err(format!(
                "HumanTask parent cancellation time is invalid: authority={requested_at}, task={}, request={}, cancelled={}",
                task.task.created_at,
                cancellation.request_event_at,
                cancellation.cancelled_event_at
            ));
        }
        if cancellation.request_reason != run.run.cancellation_reason
            || cancellation.cancelled_reason != run.run.cancellation_reason
        {
            return Err("HumanTask parent cancellation reason drifted".into());
        }
        if run.run.last_flow_sequence != cancellation.cancelled_sequence
            || run.run.updated_at != cancellation.cancelled_event_at
            || run.run.finished_at != Some(cancellation.cancelled_event_at)
        {
            return Err(format!(
                "HumanTask parent cancellation projection is invalid: run_sequence={}, event_sequence={}, run_updated={}, run_finished={:?}, event_at={}",
                run.run.last_flow_sequence,
                cancellation.cancelled_sequence,
                run.run.updated_at,
                run.run.finished_at,
                cancellation.cancelled_event_at
            ));
        }

        let evidence = HumanTaskCancellationAuthorityEvidence {
            api_version: HUMAN_TASK_CANCELLATION_AUTHORITY_API_VERSION,
            organization_id: task.task.organization_id,
            project_id: task.task.project_id,
            workflow_run_id: task.task.workflow_run_id,
            plan_revision_id: run.run.plan_revision_id,
            plan_digest: &run.run.plan_digest,
            execution_input_digest: &run.run.execution_input_digest,
            cancellation_requested_by: requested_by,
            cancellation_requested_at: requested_at,
            cancellation_reason: &run.run.cancellation_reason,
            human_task_id: task.task.id,
            task_version: task.task.aggregate_version,
            task_status: task.task.status,
            task_updated_at: task.task.updated_at,
            step_id: &task.task.step_id,
            step_attempt: task.task.step_attempt,
            configuration_digest: &hook.configuration_digest,
            flow_run_id: &task.task.flow_run_id,
            flow_hook_id: &task.task.flow_hook_id,
            hook_event_sequence: task.hook_event_sequence,
            hook_event_id: task.hook_event_id,
            request_sequence: cancellation.request_sequence,
            request_event_id: cancellation.request_event_id,
            request_event_at: cancellation.request_event_at,
            cancelled_sequence: cancellation.cancelled_sequence,
            cancelled_event_id: cancellation.cancelled_event_id,
            cancelled_event_at: cancellation.cancelled_event_at,
        };
        let canonical = canonical_json_bounded(
            &evidence,
            HUMAN_TASK_DEADLINE_EVIDENCE_MAX_BYTES,
            "HumanTask parent cancellation authority evidence",
        )?;
        let digest = Sha256Digest::parse(sha256_digest(&canonical))?;
        let authorization_decision = AuthorizationDecisionRef::new(
            format!(
                "urn:a3s:cloud:workflow:human-task-parent-cancellation:{}:v{}:{}",
                task.task.id, task.task.aggregate_version, cancellation.cancelled_event_id
            ),
            digest,
        )?;

        Ok(Self {
            decided_by: requested_by,
            authorization_decision,
            decided_at: cancellation.cancelled_event_at,
        })
    }
}

fn exact_human_task_authority(
    run: &WorkflowRunRecord,
    task: &HumanTaskRecord,
) -> Result<(ResolvedWorkflowRunStep, WorkflowHumanDecisionHookMetadata), String> {
    run.validate()?;
    task.validate()?;
    if task.task.status.is_terminal()
        || task.task.organization_id != run.run.organization_id
        || task.task.project_id != run.run.project_id
        || task.task.workflow_run_id != run.run.id
        || task.task.flow_run_id != run.run.flow_run_id
    {
        return Err("HumanTask authority does not match its WorkflowRun".into());
    }

    let step = run
        .run
        .execution_input
        .resolved_steps()?
        .into_iter()
        .find(|step| step.plan.id == task.task.step_id)
        .ok_or_else(|| "HumanTask authority cannot find its workflow step".to_owned())?;
    let hook = WorkflowHumanDecisionHookMetadata::from_run_step(&run.run.execution_input, &step)?;
    let expected_assignment = AssignmentPolicyRef::workflow_organization_member_exclusive()?;
    if step.plan.kind != WorkflowStepKind::HumanDecision
        || hook.step_attempt != task.task.step_attempt
        || hook.flow_hook_id() != task.task.flow_hook_id
        || hook.form_id.to_string() != task.task.form_release.form_id
        || hook.form_release_id.to_string() != task.task.form_release.release_id
        || hook.form_release_digest.as_str() != task.task.form_release.digest
        || task.task.assignment_policy != expected_assignment
    {
        return Err("HumanTask authority drifted from its exact workflow step".into());
    }
    Ok((step, hook))
}

pub fn expected_human_task_expiry(
    record: &WorkflowRunRecord,
    step: &ResolvedWorkflowRunStep,
    created_at: DateTime<Utc>,
) -> Result<DateTime<Utc>, String> {
    if step.plan.kind != WorkflowStepKind::HumanDecision {
        return Err("HumanTask expiry requires a human-decision step".into());
    }
    let created_at = canonical_timestamp(created_at);
    let configured = step
        .configuration
        .expires_after_seconds
        .map(|seconds| {
            i64::try_from(seconds)
                .map(chrono::Duration::seconds)
                .map_err(|_| "human-decision expiry exceeds the supported range".to_owned())
                .and_then(|duration| {
                    created_at.checked_add_signed(duration).ok_or_else(|| {
                        "human-decision expiry is outside the supported timestamp range".to_owned()
                    })
                })
        })
        .transpose()?;
    let expires_at = configured
        .map(|value| value.min(record.run.execution_input.deadline_at))
        .unwrap_or(record.run.execution_input.deadline_at);
    if expires_at <= created_at {
        return Err("human-decision hook was observed after its deadline".into());
    }
    Ok(canonical_timestamp(expires_at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::HumanTaskId;
    use crate::modules::workflow::domain::{
        HumanTask, HumanTaskInteractionSpec, NewHumanTask, WorkflowRun,
    };
    use crate::modules::workflow::test_support::{
        human_decision_form_release, human_decision_workflow_run_input, timestamp,
        TEST_HUMAN_STEP_ID,
    };

    #[test]
    fn derives_stable_exact_deadline_authority_and_rejects_drift() {
        let input = human_decision_workflow_run_input().expect("workflow input");
        let requested_by = PrincipalId::new();
        let (run, steps) = WorkflowRun::create(input.clone(), requested_by).expect("WorkflowRun");
        let run = WorkflowRunRecord { run, steps };
        let resolved = input.resolved_steps().expect("resolved steps");
        let step = resolved
            .iter()
            .find(|step| step.plan.id == TEST_HUMAN_STEP_ID)
            .expect("human step");
        let created_at = timestamp(8, 0) + chrono::Duration::seconds(1);
        let expires_at = expected_human_task_expiry(&run, step, created_at).expect("expiry");
        let task = HumanTask::create(NewHumanTask {
            organization_id: input.organization_id,
            project_id: input.project_id,
            id: HumanTaskId::new(),
            workflow_run_id: input.workflow_run_id,
            step_id: TEST_HUMAN_STEP_ID.into(),
            step_attempt: 1,
            form_release: human_decision_form_release(&input).expect("FormRelease"),
            assignment_policy: AssignmentPolicyRef::workflow_organization_member_exclusive()
                .expect("assignment"),
            flow_run_id: input.workflow_run_id.to_string(),
            flow_hook_id: format!("workflow-human:{TEST_HUMAN_STEP_ID}:1"),
            due_at: None,
            expires_at: Some(expires_at),
            created_at,
        })
        .expect("HumanTask");
        let record = HumanTaskRecord::create(
            task,
            HumanTaskInteractionSpec::approval("Approve?", None, None).expect("interaction"),
            7,
            uuid::Uuid::now_v7(),
        )
        .expect("record");

        let first = HumanTaskDeadlineAuthority::derive(&run, &record).expect("authority");
        let second = HumanTaskDeadlineAuthority::derive(&run, &record).expect("authority replay");
        assert_eq!(first, second);
        assert_eq!(first.decided_by, requested_by);
        assert_eq!(first.decided_at, expires_at);

        let mut drifted = record;
        drifted.task.expires_at = Some(expires_at - chrono::Duration::seconds(1));
        assert!(HumanTaskDeadlineAuthority::derive(&run, &drifted).is_err());
    }

    #[test]
    fn derives_parent_cancellation_from_exact_run_and_flow_evidence() {
        let input = human_decision_workflow_run_input().expect("workflow input");
        let requested_by = PrincipalId::new();
        let cancelled_by = PrincipalId::new();
        let (mut run, steps) =
            WorkflowRun::create(input.clone(), requested_by).expect("WorkflowRun");
        run.project_flow(crate::modules::workflow::domain::WorkflowRunFlowState {
            status: WorkflowRunStatus::Running,
            flow_runtime_build_id: "cloud-flow-test-build".into(),
            last_flow_sequence: 8,
            output: None,
            error: None,
            started_at: Some(timestamp(8, 1)),
            finished_at: None,
            observed_at: timestamp(8, 1),
        })
        .expect("running projection");
        run.request_cancellation(
            Some("operator request".into()),
            cancelled_by,
            timestamp(8, 2),
        )
        .expect("cancellation request");
        run.project_flow(crate::modules::workflow::domain::WorkflowRunFlowState {
            status: WorkflowRunStatus::Cancelled,
            flow_runtime_build_id: "cloud-flow-test-build".into(),
            last_flow_sequence: 10,
            output: None,
            error: None,
            started_at: Some(timestamp(8, 1)),
            finished_at: Some(timestamp(8, 4)),
            observed_at: timestamp(8, 4),
        })
        .expect("cancelled projection");
        let run = WorkflowRunRecord { run, steps };
        let resolved = input.resolved_steps().expect("resolved steps");
        let step = resolved
            .iter()
            .find(|step| step.plan.id == TEST_HUMAN_STEP_ID)
            .expect("human step");
        let created_at = timestamp(8, 0) + chrono::Duration::seconds(1);
        let expires_at = expected_human_task_expiry(&run, step, created_at).expect("expiry");
        let task = HumanTask::create(NewHumanTask {
            organization_id: input.organization_id,
            project_id: input.project_id,
            id: HumanTaskId::new(),
            workflow_run_id: input.workflow_run_id,
            step_id: TEST_HUMAN_STEP_ID.into(),
            step_attempt: 1,
            form_release: human_decision_form_release(&input).expect("FormRelease"),
            assignment_policy: AssignmentPolicyRef::workflow_organization_member_exclusive()
                .expect("assignment"),
            flow_run_id: input.workflow_run_id.to_string(),
            flow_hook_id: format!("workflow-human:{TEST_HUMAN_STEP_ID}:1"),
            due_at: None,
            expires_at: Some(expires_at),
            created_at,
        })
        .expect("HumanTask");
        let record = HumanTaskRecord::create(
            task,
            HumanTaskInteractionSpec::approval("Approve?", None, None).expect("interaction"),
            7,
            Uuid::now_v7(),
        )
        .expect("record");
        let cancellation = HumanTaskParentCancellationEvidence {
            flow_run_id: input.workflow_run_id.to_string(),
            request_sequence: 9,
            request_event_id: Uuid::now_v7(),
            request_event_at: timestamp(8, 3),
            request_reason: Some("operator request".into()),
            cancelled_sequence: 10,
            cancelled_event_id: Uuid::now_v7(),
            cancelled_event_at: timestamp(8, 4),
            cancelled_reason: Some("operator request".into()),
        };

        let first = HumanTaskCancellationAuthority::derive(&run, &record, &cancellation)
            .expect("cancellation authority");
        let second = HumanTaskCancellationAuthority::derive(&run, &record, &cancellation)
            .expect("cancellation authority replay");
        assert_eq!(first, second);
        assert_eq!(first.decided_by, cancelled_by);
        assert_eq!(first.decided_at, timestamp(8, 4));

        let mut drifted = cancellation;
        drifted.cancelled_reason = Some("different reason".into());
        assert!(HumanTaskCancellationAuthority::derive(&run, &record, &drifted).is_err());
    }
}
