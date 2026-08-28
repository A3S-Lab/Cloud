use crate::modules::workflow::domain::HumanTaskRecord;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanTaskStateChanged {
    pub project_id: Uuid,
    pub workflow_run_id: Uuid,
    pub human_task_id: Uuid,
    pub step_id: String,
    pub step_attempt: u64,
    pub form_id: String,
    pub form_release_id: String,
    pub assignment_policy_id: String,
    pub assignment_policy_revision: u64,
    pub assignment_policy_digest: String,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub status: String,
}

impl HumanTaskStateChanged {
    pub fn envelope(
        record: &HumanTaskRecord,
        causation_id: Option<Uuid>,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let task = &record.task;
        let event_key = match task.status {
            super::super::HumanTaskStatus::PendingActivation => "workflow.human-task.created",
            super::super::HumanTaskStatus::Ready => "workflow.human-task.ready",
            super::super::HumanTaskStatus::Claimed => "workflow.human-task.claimed",
            super::super::HumanTaskStatus::Completed => "workflow.human-task.completed",
            super::super::HumanTaskStatus::Expired => "workflow.human-task.expired",
            super::super::HumanTaskStatus::Cancelled => "workflow.human-task.cancelled",
        };
        let event_identity = format!("{event_key}:{}", task.aggregate_version);
        Ok(DomainEventEnvelope {
            event_id: Uuid::new_v5(&task.id.as_uuid(), event_identity.as_bytes()),
            event_key: event_key.into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: task.organization_id.as_uuid(),
            },
            aggregate_id: task.id.as_uuid(),
            aggregate_version: task.aggregate_version,
            occurred_at: task.updated_at,
            correlation_id: task.workflow_run_id.as_uuid(),
            causation_id,
            payload: serde_json::to_value(Self {
                project_id: task.project_id.as_uuid(),
                workflow_run_id: task.workflow_run_id.as_uuid(),
                human_task_id: task.id.as_uuid(),
                step_id: task.step_id.clone(),
                step_attempt: task.step_attempt,
                form_id: task.form_release.form_id.clone(),
                form_release_id: task.form_release.release_id.clone(),
                assignment_policy_id: task.assignment_policy.id.clone(),
                assignment_policy_revision: task.assignment_policy.revision,
                assignment_policy_digest: task.assignment_policy.digest.to_string(),
                flow_run_id: task.flow_run_id.clone(),
                flow_hook_id: task.flow_hook_id.clone(),
                status: task.status.as_str().into(),
            })?,
        })
    }
}
