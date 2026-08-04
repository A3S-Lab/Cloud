use super::AgentEventContent;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AgentConversationId, AgentExecutionId, OrganizationId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const MAX_AGENT_EVENTS_PER_APPEND: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentExecutionEventKind {
    ExecutionRequested,
    ModelOutput,
    ExecutionFailed,
    ExecutionCompleted,
}

impl AgentExecutionEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExecutionRequested => "execution_requested",
            Self::ModelOutput => "model_output",
            Self::ExecutionFailed => "execution_failed",
            Self::ExecutionCompleted => "execution_completed",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "execution_requested" => Ok(Self::ExecutionRequested),
            "model_output" => Ok(Self::ModelOutput),
            "execution_failed" => Ok(Self::ExecutionFailed),
            "execution_completed" => Ok(Self::ExecutionCompleted),
            _ => Err(format!("unsupported Agent execution event kind {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionEventDraft {
    pub kind: AgentExecutionEventKind,
    pub content: AgentEventContent,
    pub occurred_at: DateTime<Utc>,
}

impl AgentExecutionEventDraft {
    pub fn new(
        kind: AgentExecutionEventKind,
        content: AgentEventContent,
        occurred_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        content.validate()?;
        Ok(Self {
            kind,
            content,
            occurred_at: canonical_timestamp(occurred_at),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentExecutionEvent {
    pub organization_id: OrganizationId,
    pub conversation_id: AgentConversationId,
    pub execution_id: AgentExecutionId,
    pub sequence: u64,
    pub kind: AgentExecutionEventKind,
    pub content: AgentEventContent,
    pub occurred_at: DateTime<Utc>,
}

impl AgentExecutionEvent {
    pub fn from_draft(
        organization_id: OrganizationId,
        conversation_id: AgentConversationId,
        execution_id: AgentExecutionId,
        sequence: u64,
        draft: AgentExecutionEventDraft,
    ) -> Result<Self, String> {
        let event = Self {
            organization_id,
            conversation_id,
            execution_id,
            sequence,
            kind: draft.kind,
            content: draft.content,
            occurred_at: canonical_timestamp(draft.occurred_at),
        };
        event.validate()?;
        Ok(event)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.content.validate()?;
        if self.organization_id.as_uuid().is_nil()
            || self.conversation_id.as_uuid().is_nil()
            || self.execution_id.as_uuid().is_nil()
            || self.sequence == 0
            || self.occurred_at != canonical_timestamp(self.occurred_at)
        {
            return Err("Agent execution event is invalid".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_requires_a_positive_sequence() {
        let draft = AgentExecutionEventDraft::new(
            AgentExecutionEventKind::ExecutionRequested,
            AgentEventContent::inline_json(serde_json::json!({})).expect("content"),
            Utc::now(),
        )
        .expect("draft");
        assert!(AgentExecutionEvent::from_draft(
            OrganizationId::new(),
            AgentConversationId::new(),
            AgentExecutionId::new(),
            0,
            draft,
        )
        .is_err());
    }
}
