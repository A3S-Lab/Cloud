use crate::modules::workflow::domain::{PlanRevision, WorkflowGoal};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowGoalCompiled {
    pub project_id: Uuid,
    pub workflow_goal_id: Uuid,
    pub plan_revision_id: Uuid,
    pub workflow_definition_id: Uuid,
    pub workflow_revision_id: Uuid,
    pub ontology_id: Uuid,
    pub ontology_revision_id: Uuid,
    pub input_digest: String,
    pub plan_digest: String,
    pub compiler_revision: String,
}

impl WorkflowGoalCompiled {
    pub fn envelope(
        goal: &WorkflowGoal,
        plan: &PlanRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            project_id: goal.project_id.as_uuid(),
            workflow_goal_id: goal.id.as_uuid(),
            plan_revision_id: plan.id.as_uuid(),
            workflow_definition_id: plan.plan.workflow_definition_id.as_uuid(),
            workflow_revision_id: plan.plan.workflow_revision_id.as_uuid(),
            ontology_id: plan.plan.ontology_id.as_uuid(),
            ontology_revision_id: plan.plan.ontology_revision_id.as_uuid(),
            input_digest: plan.plan.input_digest.as_str().to_owned(),
            plan_digest: plan.digest.as_str().to_owned(),
            compiler_revision: plan.plan.compiler_revision.clone(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "workflow.goal.compiled".into(),
            schema_version: 1,
            organization_id: goal.organization_id.as_uuid(),
            aggregate_id: goal.id.as_uuid(),
            aggregate_version: 1,
            occurred_at: goal.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
