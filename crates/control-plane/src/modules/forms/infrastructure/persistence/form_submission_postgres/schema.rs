use a3s_orm::orm_table;
use chrono::{DateTime, Utc};
use uuid::Uuid;

orm_table! {
    pub(super) struct FormSubmissions => "form_submissions" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        id: Uuid => "id",
        workflow_run_id: Uuid => "workflow_run_id",
        human_task_id: Uuid => "human_task_id",
        form_id: Uuid => "form_id",
        form_release_id: Uuid => "form_release_id",
        flow_run_id: String => "flow_run_id",
        flow_hook_id: String => "flow_hook_id",
        step_id: String => "step_id",
        step_attempt: u64 => "step_attempt",
        principal_id: Uuid => "principal_id",
        authorization_decision_id: String => "authorization_decision_id",
        authorization_decision_digest: String => "authorization_decision_digest",
        outcome: String => "outcome",
        interaction_request_digest: String => "interaction_request_digest",
        interaction_submission_id: String => "interaction_submission_id",
        idempotency_key: String => "idempotency_key",
        candidate_value_digest: String => "candidate_value_digest",
        output_digest: String => "output_digest",
        digest: String => "digest",
        aggregate_version: u64 => "aggregate_version",
        record_json: String => "record_json",
        submitted_at: DateTime<Utc> => "submitted_at",
        accepted_at: DateTime<Utc> => "accepted_at",
    }
}
