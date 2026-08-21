use crate::modules::applications::application::{
    ApplicationInvocationMutationResult, ApplicationSessionMutationResult,
    ApplicationWorkflowRunEvidence,
};
use crate::modules::applications::domain::{
    ApplicationInvocation, ApplicationMessage, ApplicationSession, ApplicationWorkflowEffect,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenApplicationSessionRequest {
    pub release_id: Uuid,
    #[serde(default = "empty_object")]
    pub initial_variables: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestApplicationInvocationRequest {
    pub ontology_id: Uuid,
    pub ontology_revision_id: Uuid,
    pub environment_id: Option<Uuid>,
    pub response_mode: String,
    pub input: Value,
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSessionResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub application_id: Uuid,
    pub application_release_id: Uuid,
    pub application_release_number: u64,
    pub application_release_digest: String,
    pub end_user_id: Uuid,
    pub session_id: Uuid,
    pub interaction_mode: String,
    pub status: String,
    pub last_message_sequence: u64,
    pub current_variable_revision_id: Uuid,
    pub current_variable_revision_number: u64,
    pub current_variable_digest: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}

impl From<ApplicationSession> for ApplicationSessionResponse {
    fn from(session: ApplicationSession) -> Self {
        Self {
            organization_id: session.organization_id.as_uuid(),
            project_id: session.project_id.as_uuid(),
            application_id: session.application_id.as_uuid(),
            application_release_id: session.application_release_id.as_uuid(),
            application_release_number: session.application_release_number,
            application_release_digest: session.application_release_digest.as_str().to_owned(),
            end_user_id: session.end_user_id.as_uuid(),
            session_id: session.id.as_uuid(),
            interaction_mode: session.interaction_mode.as_str().to_owned(),
            status: session.status.as_str().to_owned(),
            last_message_sequence: session.last_message_sequence,
            current_variable_revision_id: session.current_variable_revision_id.as_uuid(),
            current_variable_revision_number: session.current_variable_revision_number,
            current_variable_digest: session.current_variable_digest.as_str().to_owned(),
            aggregate_version: session.aggregate_version,
            created_at: session.created_at,
            updated_at: session.updated_at,
            closed_at: session.closed_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSessionMutationResponse {
    pub session: ApplicationSessionResponse,
    pub replayed: bool,
}

impl From<ApplicationSessionMutationResult> for ApplicationSessionMutationResponse {
    fn from(result: ApplicationSessionMutationResult) -> Self {
        Self {
            session: result.session.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationInvocationResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub application_id: Uuid,
    pub application_release_id: Uuid,
    pub application_release_digest: String,
    pub session_id: Uuid,
    pub invocation_id: Uuid,
    pub response_mode: String,
    pub input: Value,
    pub input_digest: String,
    pub workflow_run_id: Option<Uuid>,
    pub status: String,
    pub aggregate_version: u64,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl From<ApplicationInvocation> for ApplicationInvocationResponse {
    fn from(invocation: ApplicationInvocation) -> Self {
        Self {
            organization_id: invocation.organization_id.as_uuid(),
            project_id: invocation.project_id.as_uuid(),
            application_id: invocation.application_id.as_uuid(),
            application_release_id: invocation.application_release_id.as_uuid(),
            application_release_digest: invocation.application_release_digest.as_str().to_owned(),
            session_id: invocation.session_id.as_uuid(),
            invocation_id: invocation.id.as_uuid(),
            response_mode: invocation.response_mode.as_str().to_owned(),
            input: invocation.input,
            input_digest: invocation.input_digest.as_str().to_owned(),
            workflow_run_id: invocation.workflow_run_id.map(|value| value.as_uuid()),
            status: invocation.status.as_str().to_owned(),
            aggregate_version: invocation.aggregate_version,
            requested_at: invocation.requested_at,
            updated_at: invocation.updated_at,
            completed_at: invocation.completed_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationWorkflowRunEvidenceResponse {
    pub workflow_run_id: Uuid,
    pub workflow_goal_id: Uuid,
    pub plan_revision_id: Uuid,
    pub plan_digest: String,
    pub ontology_id: Uuid,
    pub ontology_revision_id: Uuid,
    pub ontology_digest: String,
    pub environment_id: Option<Uuid>,
    pub requested_at: DateTime<Utc>,
    pub deadline_at: DateTime<Utc>,
}

impl From<ApplicationWorkflowRunEvidence> for ApplicationWorkflowRunEvidenceResponse {
    fn from(evidence: ApplicationWorkflowRunEvidence) -> Self {
        Self {
            workflow_run_id: evidence.workflow_run_id.as_uuid(),
            workflow_goal_id: evidence.workflow_goal_id.as_uuid(),
            plan_revision_id: evidence.plan_revision_id.as_uuid(),
            plan_digest: evidence.plan_digest.as_str().to_owned(),
            ontology_id: evidence.ontology_id.as_uuid(),
            ontology_revision_id: evidence.ontology_revision_id.as_uuid(),
            ontology_digest: evidence.ontology_digest.as_str().to_owned(),
            environment_id: evidence.environment_id.map(|value| value.as_uuid()),
            requested_at: evidence.requested_at,
            deadline_at: evidence.deadline_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationInvocationMutationResponse {
    pub invocation: ApplicationInvocationResponse,
    pub workflow: ApplicationWorkflowRunEvidenceResponse,
    pub replayed: bool,
}

impl From<ApplicationInvocationMutationResult> for ApplicationInvocationMutationResponse {
    fn from(result: ApplicationInvocationMutationResult) -> Self {
        Self {
            invocation: result.invocation.into(),
            workflow: result.workflow.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationWorkflowEffectResponse {
    pub workflow_run_id: Uuid,
    pub step_id: String,
    pub attempt: u32,
    pub ordinal: u32,
}

impl From<ApplicationWorkflowEffect> for ApplicationWorkflowEffectResponse {
    fn from(effect: ApplicationWorkflowEffect) -> Self {
        Self {
            workflow_run_id: effect.workflow_run_id.as_uuid(),
            step_id: effect.step_id,
            attempt: effect.attempt,
            ordinal: effect.ordinal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationMessageResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub application_id: Uuid,
    pub application_release_id: Uuid,
    pub application_release_digest: String,
    pub session_id: Uuid,
    pub invocation_id: Uuid,
    pub message_id: Uuid,
    pub sequence: u64,
    pub kind: String,
    pub content: Value,
    pub content_digest: String,
    pub workflow_effect: Option<ApplicationWorkflowEffectResponse>,
    pub created_at: DateTime<Utc>,
}

impl From<ApplicationMessage> for ApplicationMessageResponse {
    fn from(message: ApplicationMessage) -> Self {
        Self {
            organization_id: message.organization_id.as_uuid(),
            project_id: message.project_id.as_uuid(),
            application_id: message.application_id.as_uuid(),
            application_release_id: message.application_release_id.as_uuid(),
            application_release_digest: message.application_release_digest.as_str().to_owned(),
            session_id: message.session_id.as_uuid(),
            invocation_id: message.invocation_id.as_uuid(),
            message_id: message.id.as_uuid(),
            sequence: message.sequence,
            kind: message.kind.as_str().to_owned(),
            content: message.content,
            content_digest: message.content_digest.as_str().to_owned(),
            workflow_effect: message.workflow_effect.map(Into::into),
            created_at: message.created_at,
        }
    }
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}
