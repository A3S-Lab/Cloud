use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, AuthorizationDecisionRef,
    HumanTaskId, OrganizationId, PlanRevisionId, PrincipalId, ProjectId, Sha256Digest,
    WorkflowRunId,
};
use crate::modules::workflow::domain::{
    AssignmentPolicyRef, HumanTaskRecord, HumanTaskStatus, ResolvedWorkflowRunStep,
    WorkflowHumanDecisionHookMetadata, WorkflowRunRecord, WorkflowStepKind,
};
use chrono::{DateTime, Utc};
use serde::Serialize;

pub const HUMAN_TASK_DEADLINE_AUTHORITY_API_VERSION: &str =
    "a3s.dev/workflow/human-task-deadline-authority/v1";
const HUMAN_TASK_DEADLINE_EVIDENCE_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanTaskDeadlineAuthority {
    pub decided_by: PrincipalId,
    pub authorization_decision: AuthorizationDecisionRef,
    pub decided_at: DateTime<Utc>,
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

impl HumanTaskDeadlineAuthority {
    pub fn derive(run: &WorkflowRunRecord, task: &HumanTaskRecord) -> Result<Self, String> {
        run.validate()?;
        task.validate()?;
        if task.task.status.is_terminal()
            || task.task.organization_id != run.run.organization_id
            || task.task.project_id != run.run.project_id
            || task.task.workflow_run_id != run.run.id
            || task.task.flow_run_id != run.run.flow_run_id
        {
            return Err("HumanTask deadline authority does not match its WorkflowRun".into());
        }

        let resolved = run.run.execution_input.resolved_steps()?;
        let step = resolved
            .iter()
            .find(|step| step.plan.id == task.task.step_id)
            .ok_or_else(|| {
                "HumanTask deadline authority cannot find its workflow step".to_owned()
            })?;
        let hook =
            WorkflowHumanDecisionHookMetadata::from_run_step(&run.run.execution_input, step)?;
        let expected_assignment = AssignmentPolicyRef::workflow_organization_member_exclusive()?;
        if step.plan.kind != WorkflowStepKind::HumanDecision
            || hook.step_attempt != task.task.step_attempt
            || hook.flow_hook_id() != task.task.flow_hook_id
            || hook.form_id.to_string() != task.task.form_release.form_id
            || hook.form_release_id.to_string() != task.task.form_release.release_id
            || hook.form_release_digest.as_str() != task.task.form_release.digest
            || task.task.assignment_policy != expected_assignment
        {
            return Err("HumanTask deadline authority drifted from its exact workflow step".into());
        }

        let expires_at = expected_human_task_expiry(run, step, task.task.created_at)?;
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
}
