use crate::modules::workflow::application::HumanTaskMutationResult;
use crate::modules::workflow::domain::HumanTaskRecord;
use a3s_form_core::{
    CanonicalValue, FormInteractionOutcome, FormInteractionOutputMapping, FormInteractionRequest,
    FormReleaseRef,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanTaskAssignmentPolicyResponse {
    pub id: String,
    pub revision: u64,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanTaskSummaryResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub id: Uuid,
    pub workflow_run_id: Uuid,
    pub step_id: String,
    pub step_attempt: u64,
    pub form_release: FormReleaseRef,
    pub assignment_policy: HumanTaskAssignmentPolicyResponse,
    pub status: String,
    pub claimed_by: Option<Uuid>,
    pub decision_id: Option<Uuid>,
    pub aggregate_version: u64,
    pub message: String,
    pub allowed_outcomes: Vec<FormInteractionOutcome>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub due_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub terminal_at: Option<DateTime<Utc>>,
}

impl From<&HumanTaskRecord> for HumanTaskSummaryResponse {
    fn from(value: &HumanTaskRecord) -> Self {
        Self {
            organization_id: value.task.organization_id.as_uuid(),
            project_id: value.task.project_id.as_uuid(),
            id: value.task.id.as_uuid(),
            workflow_run_id: value.task.workflow_run_id.as_uuid(),
            step_id: value.task.step_id.clone(),
            step_attempt: value.task.step_attempt,
            form_release: value.task.form_release.clone(),
            assignment_policy: HumanTaskAssignmentPolicyResponse {
                id: value.task.assignment_policy.id.clone(),
                revision: value.task.assignment_policy.revision,
                digest: value.task.assignment_policy.digest.to_string(),
            },
            status: value.task.status.as_str().to_owned(),
            claimed_by: value.task.claimed_by.map(|id| id.as_uuid()),
            decision_id: value.task.decision_id.map(|id| id.as_uuid()),
            aggregate_version: value.task.aggregate_version,
            message: value.interaction.message.clone(),
            allowed_outcomes: value.interaction.allowed_outcomes.clone(),
            created_at: value.task.created_at,
            updated_at: value.task.updated_at,
            due_at: value.task.due_at,
            expires_at: value.task.expires_at,
            claimed_at: value.task.claimed_at,
            terminal_at: value.task.terminal_at,
        }
    }
}

impl From<HumanTaskRecord> for HumanTaskSummaryResponse {
    fn from(value: HumanTaskRecord) -> Self {
        Self::from(&value)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanTaskResponse {
    #[serde(flatten)]
    pub summary: HumanTaskSummaryResponse,
    pub details: Option<String>,
    pub output_mapping: FormInteractionOutputMapping,
    pub max_value_bytes: u64,
    pub initial_value: Option<CanonicalValue>,
    pub interaction_request: Option<FormInteractionRequest>,
}

impl From<HumanTaskRecord> for HumanTaskResponse {
    fn from(value: HumanTaskRecord) -> Self {
        let summary = HumanTaskSummaryResponse::from(&value);
        Self {
            summary,
            details: value.interaction.details,
            output_mapping: value.interaction.output_mapping,
            max_value_bytes: value.interaction.max_value_bytes,
            initial_value: value.interaction.initial_value,
            interaction_request: value.interaction_request,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanTaskMutationResponse {
    pub human_task: HumanTaskResponse,
    pub replayed: bool,
}

impl From<HumanTaskMutationResult> for HumanTaskMutationResponse {
    fn from(value: HumanTaskMutationResult) -> Self {
        Self {
            human_task: HumanTaskResponse::from(value.record),
            replayed: value.replayed,
        }
    }
}
