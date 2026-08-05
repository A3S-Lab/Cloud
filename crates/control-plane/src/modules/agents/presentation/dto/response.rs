use crate::modules::agents::application::{
    AgentExecutionEventPage, CancelAgentExecutionResult, CreateAgentConversationResult,
    StartAgentExecutionResult,
};
use crate::modules::agents::domain::{
    AgentConversation, AgentConversationStatus, AgentExecution, AgentExecutionEvent,
    AgentExecutionEventKind, AgentExecutionStatus,
};
use crate::presentation::{format_sequence_cursor, SequencePage, SequenceRecord};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationMutationResponse {
    pub conversation: AgentConversationResponse,
    pub replayed: bool,
}

impl From<CreateAgentConversationResult> for AgentConversationMutationResponse {
    fn from(result: CreateAgentConversationResult) -> Self {
        Self {
            conversation: result.conversation.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionMutationResponse {
    pub conversation: AgentConversationResponse,
    pub execution: AgentExecutionResponse,
    pub replayed: bool,
}

impl From<StartAgentExecutionResult> for AgentExecutionMutationResponse {
    fn from(result: StartAgentExecutionResult) -> Self {
        Self {
            conversation: result.conversation.into(),
            execution: result.execution.into(),
            replayed: result.replayed,
        }
    }
}

impl From<CancelAgentExecutionResult> for AgentExecutionMutationResponse {
    fn from(result: CancelAgentExecutionResult) -> Self {
        Self {
            conversation: result.conversation.into(),
            execution: result.execution.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub id: Uuid,
    pub status: AgentConversationStatus,
    pub last_event_sequence: u64,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}

impl From<AgentConversation> for AgentConversationResponse {
    fn from(conversation: AgentConversation) -> Self {
        Self {
            organization_id: conversation.organization_id.as_uuid(),
            project_id: conversation.project_id.as_uuid(),
            environment_id: conversation.environment_id.as_uuid(),
            id: conversation.id.as_uuid(),
            status: conversation.status,
            last_event_sequence: conversation.last_event_sequence,
            aggregate_version: conversation.aggregate_version,
            created_at: conversation.created_at,
            updated_at: conversation.updated_at,
            closed_at: conversation.closed_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionResponse {
    pub organization_id: Uuid,
    pub conversation_id: Uuid,
    pub id: Uuid,
    pub operation_id: Uuid,
    pub agent: AgentReleaseBindingResponse,
    pub status: AgentExecutionStatus,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub cancellation_requested_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl From<AgentExecution> for AgentExecutionResponse {
    fn from(execution: AgentExecution) -> Self {
        Self {
            organization_id: execution.organization_id.as_uuid(),
            conversation_id: execution.conversation_id.as_uuid(),
            id: execution.id.as_uuid(),
            operation_id: execution.operation_id.as_uuid(),
            agent: AgentReleaseBindingResponse {
                asset_id: execution.agent.asset_id().as_uuid(),
                asset_release_id: execution.agent.asset_release_id().as_uuid(),
                build_run_id: execution.agent.build_run_id().as_uuid(),
                artifact_uri: execution.agent.artifact_uri().to_owned(),
                artifact_digest: execution.agent.artifact_digest().as_str().to_owned(),
                artifact_media_type: execution.agent.artifact_media_type().to_owned(),
                artifact_size_bytes: execution.agent.artifact_size_bytes(),
            },
            status: execution.status,
            failure: execution.failure,
            aggregate_version: execution.aggregate_version,
            requested_at: execution.requested_at,
            updated_at: execution.updated_at,
            started_at: execution.started_at,
            cancellation_requested_at: execution.cancellation_requested_at,
            finished_at: execution.finished_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentReleaseBindingResponse {
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub build_run_id: Uuid,
    pub artifact_uri: String,
    pub artifact_digest: String,
    pub artifact_media_type: String,
    pub artifact_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionEventResponse {
    pub organization_id: Uuid,
    pub conversation_id: Uuid,
    pub execution_id: Uuid,
    pub sequence: u64,
    pub kind: AgentExecutionEventKind,
    pub content: serde_json::Value,
    pub content_digest: String,
    pub content_size_bytes: u64,
    pub occurred_at: DateTime<Utc>,
}

impl From<AgentExecutionEvent> for AgentExecutionEventResponse {
    fn from(event: AgentExecutionEvent) -> Self {
        Self {
            organization_id: event.organization_id.as_uuid(),
            conversation_id: event.conversation_id.as_uuid(),
            execution_id: event.execution_id.as_uuid(),
            sequence: event.sequence,
            kind: event.kind,
            content: event.content.value().clone(),
            content_digest: event.content.digest().as_str().to_owned(),
            content_size_bytes: event.content.size_bytes(),
            occurred_at: event.occurred_at,
        }
    }
}

impl SequenceRecord for AgentExecutionEventResponse {
    fn sequence(&self) -> u64 {
        self.sequence
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionEventPageResponse {
    pub conversation_id: Uuid,
    pub head_sequence: u64,
    pub records: Vec<AgentExecutionEventResponse>,
    pub next_cursor: Option<String>,
}

impl From<AgentExecutionEventPage> for AgentExecutionEventPageResponse {
    fn from(page: AgentExecutionEventPage) -> Self {
        Self {
            conversation_id: page.conversation_id.as_uuid(),
            head_sequence: page.head_sequence,
            records: page.records.into_iter().map(Into::into).collect(),
            next_cursor: page.next_after_sequence.map(format_sequence_cursor),
        }
    }
}

impl SequencePage for AgentExecutionEventPageResponse {
    type Record = AgentExecutionEventResponse;

    fn records(&self) -> &[Self::Record] {
        &self.records
    }

    fn take_records(&mut self) -> Vec<Self::Record> {
        std::mem::take(&mut self.records)
    }

    fn replace_records(&mut self, records: Vec<Self::Record>) {
        self.records = records;
    }

    fn set_next_cursor(&mut self, cursor: Option<String>) {
        self.next_cursor = cursor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::agents::domain::{AgentEventContent, AgentExecutionEventKind};
    use crate::modules::shared_kernel::domain::{
        AgentConversationId, AgentExecutionId, OrganizationId,
    };

    #[test]
    fn event_response_is_camel_case_and_exposes_verified_content_identity() {
        let event = AgentExecutionEvent::from_draft(
            OrganizationId::new(),
            AgentConversationId::new(),
            AgentExecutionId::new(),
            1,
            crate::modules::agents::domain::AgentExecutionEventDraft::new(
                AgentExecutionEventKind::ModelOutput,
                AgentEventContent::inline_json(serde_json::json!({"text": "hello"}))
                    .expect("content"),
                Utc::now(),
            )
            .expect("draft"),
        )
        .expect("event");
        let encoded =
            serde_json::to_value(AgentExecutionEventResponse::from(event)).expect("response");
        assert_eq!(encoded["sequence"], 1);
        assert!(encoded.get("conversationId").is_some());
        assert!(encoded.get("contentDigest").is_some());
        assert!(encoded.get("contentSizeBytes").is_some());
    }
}
