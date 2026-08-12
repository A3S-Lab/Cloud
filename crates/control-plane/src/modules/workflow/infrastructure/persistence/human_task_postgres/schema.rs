use a3s_orm::orm_table;
use chrono::{DateTime, Utc};
use uuid::Uuid;

orm_table! {
    pub(super) struct WorkflowRuns => "workflow_runs" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        id: Uuid => "id",
        status: String => "status",
    }
}

orm_table! {
    pub(super) struct FormReleases => "form_releases" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        form_id: Uuid => "form_id",
        id: Uuid => "id",
    }
}

orm_table! {
    pub(super) struct HumanTasks => "human_tasks" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        id: Uuid => "id",
        workflow_run_id: Uuid => "workflow_run_id",
        step_id: String => "step_id",
        step_attempt: u64 => "step_attempt",
        form_id: Uuid => "form_id",
        form_release_id: Uuid => "form_release_id",
        assignment_policy_id: String => "assignment_policy_id",
        assignment_policy_revision: u64 => "assignment_policy_revision",
        assignment_policy_digest: String => "assignment_policy_digest",
        flow_run_id: String => "flow_run_id",
        flow_hook_id: String => "flow_hook_id",
        status: String => "status",
        claimed_by: Option<Uuid> => "claimed_by",
        decision_id: Option<Uuid> => "decision_id",
        aggregate_version: u64 => "aggregate_version",
        task_json: String => "task_json",
        interaction_spec_json: String => "interaction_spec_json",
        interaction_request_json: Option<String> => "interaction_request_json",
        interaction_request_digest: Option<String> => "interaction_request_digest",
        hook_event_sequence: u64 => "hook_event_sequence",
        hook_event_id: Uuid => "hook_event_id",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
        due_at: Option<DateTime<Utc>> => "due_at",
        expires_at: Option<DateTime<Utc>> => "expires_at",
        claimed_at: Option<DateTime<Utc>> => "claimed_at",
        terminal_at: Option<DateTime<Utc>> => "terminal_at",
    }
}

orm_table! {
    pub(super) struct WorkflowDecisions => "workflow_decisions" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        id: Uuid => "id",
        workflow_run_id: Uuid => "workflow_run_id",
        human_task_id: Uuid => "human_task_id",
        flow_run_id: String => "flow_run_id",
        flow_hook_id: String => "flow_hook_id",
        step_id: String => "step_id",
        step_attempt: u64 => "step_attempt",
        task_version: u64 => "task_version",
        form_id: Uuid => "form_id",
        form_release_id: Uuid => "form_release_id",
        assignment_policy_id: String => "assignment_policy_id",
        assignment_policy_revision: u64 => "assignment_policy_revision",
        assignment_policy_digest: String => "assignment_policy_digest",
        outcome: String => "outcome",
        form_submission_id: Option<Uuid> => "form_submission_id",
        form_submission_digest: Option<String> => "form_submission_digest",
        decided_by: Uuid => "decided_by",
        authorization_decision_id: String => "authorization_decision_id",
        authorization_decision_digest: String => "authorization_decision_digest",
        output_digest: String => "output_digest",
        digest: String => "digest",
        record_json: String => "record_json",
        decided_at: DateTime<Utc> => "decided_at",
    }
}

orm_table! {
    pub(super) struct WorkflowHumanTaskInbox => "workflow_human_task_inbox" {
        organization_id: Uuid => "organization_id",
        workflow_run_id: Uuid => "workflow_run_id",
        flow_sequence: u64 => "flow_sequence",
        event_id: Uuid => "event_id",
        event_key: String => "event_key",
        event_digest: String => "event_digest",
        observed_at: DateTime<Utc> => "observed_at",
        processed_at: DateTime<Utc> => "processed_at",
    }
}

orm_table! {
    pub(super) struct WorkflowResumeOutbox => "workflow_resume_outbox" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        workflow_decision_id: Uuid => "workflow_decision_id",
        workflow_run_id: Uuid => "workflow_run_id",
        human_task_id: Uuid => "human_task_id",
        flow_run_id: String => "flow_run_id",
        flow_hook_id: String => "flow_hook_id",
        payload_json: String => "payload_json",
        payload_digest: String => "payload_digest",
        state: String => "state",
        attempt_count: i32 => "attempt_count",
        available_at: DateTime<Utc> => "available_at",
        lease_owner: Option<Uuid> => "lease_owner",
        lease_expires_at: Option<DateTime<Utc>> => "lease_expires_at",
        last_error: Option<String> => "last_error",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
        delivered_at: Option<DateTime<Utc>> => "delivered_at",
    }
}

orm_table! {
    pub(super) struct WorkflowResumeCandidates => "workflow_resume_candidates" {
        organization_id: Uuid => "organization_id",
        workflow_decision_id: Uuid => "workflow_decision_id",
    }
}

orm_table! {
    pub(super) struct WorkflowResumeReceipts => "workflow_resume_receipts" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        workflow_decision_id: Uuid => "workflow_decision_id",
        workflow_run_id: Uuid => "workflow_run_id",
        human_task_id: Uuid => "human_task_id",
        flow_run_id: String => "flow_run_id",
        flow_hook_id: String => "flow_hook_id",
        payload_digest: String => "payload_digest",
        disposition: String => "disposition",
        flow_event_sequence: u64 => "flow_event_sequence",
        flow_event_id: Uuid => "flow_event_id",
        flow_event_at: DateTime<Utc> => "flow_event_at",
        receipt_json: String => "receipt_json",
        recorded_at: DateTime<Utc> => "recorded_at",
    }
}
