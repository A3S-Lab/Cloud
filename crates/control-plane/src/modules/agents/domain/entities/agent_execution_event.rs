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
    ToolRequest,
    ToolResult,
    ApprovalResolved,
    ExecutionFailed,
    ExecutionCompleted,
    ExecutionCancelled,
}

impl AgentExecutionEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExecutionRequested => "execution_requested",
            Self::ModelOutput => "model_output",
            Self::ToolRequest => "tool_request",
            Self::ToolResult => "tool_result",
            Self::ApprovalResolved => "approval_resolved",
            Self::ExecutionFailed => "execution_failed",
            Self::ExecutionCompleted => "execution_completed",
            Self::ExecutionCancelled => "execution_cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "execution_requested" => Ok(Self::ExecutionRequested),
            "model_output" => Ok(Self::ModelOutput),
            "tool_request" => Ok(Self::ToolRequest),
            "tool_result" => Ok(Self::ToolResult),
            "approval_resolved" => Ok(Self::ApprovalResolved),
            "execution_failed" => Ok(Self::ExecutionFailed),
            "execution_completed" => Ok(Self::ExecutionCompleted),
            "execution_cancelled" => Ok(Self::ExecutionCancelled),
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

    /// Project only Cloud-owned conversation semantics from one exact Code
    /// event page. The Code event records themselves remain authoritative in
    /// `a3s code harness` and are not copied into another event log.
    pub fn semantic_from_code_page(
        page: &a3s_cloud_contracts::AgentProtocolEventPageV1,
        projected_at: DateTime<Utc>,
    ) -> Result<Vec<Self>, String> {
        use a3s_cloud_contracts::{AgentEventTypeV1, AgentProtocolRunStateV1};

        page.validate()
            .map_err(|error| format!("invalid A3S Code event page ({})", error.code()))?;
        let projected_at = canonical_timestamp(projected_at);
        let mut drafts = Vec::new();
        for record in &page.events {
            if record.event.event_type == AgentEventTypeV1::TEXT_DELTA {
                let text = record
                    .event
                    .payload
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "A3S Code text_delta omitted its text value".to_owned())?;
                drafts.push(Self::new(
                    AgentExecutionEventKind::ModelOutput,
                    AgentEventContent::inline_json(serde_json::json!({"text": text}))?,
                    projected_at,
                )?);
            }
        }
        if !page.state.is_terminal() || page.has_more {
            return Ok(drafts);
        }

        let (kind, content) = match page.state {
            AgentProtocolRunStateV1::Completed => (
                AgentExecutionEventKind::ExecutionCompleted,
                serde_json::json!({}),
            ),
            AgentProtocolRunStateV1::Failed => {
                let reason = page
                    .events
                    .iter()
                    .rev()
                    .find(|record| record.event.event_type == AgentEventTypeV1::ERROR)
                    .and_then(|record| record.event.payload.get("message"))
                    .and_then(serde_json::Value::as_str)
                    .map(normalize_failure_reason)
                    .unwrap_or_else(|| "A3S Code run failed".into());
                (
                    AgentExecutionEventKind::ExecutionFailed,
                    serde_json::json!({"reason": reason}),
                )
            }
            AgentProtocolRunStateV1::Cancelled => (
                AgentExecutionEventKind::ExecutionCancelled,
                serde_json::json!({}),
            ),
            _ => return Err("non-terminal A3S Code state reached terminal projection".into()),
        };
        drafts.push(Self::new(
            kind,
            AgentEventContent::inline_json(content)?,
            projected_at,
        )?);
        Ok(drafts)
    }

    /// Project the bounded provider-neutral semantic subset into the sole
    /// Cloud conversation sequence. Provider-private records remain outside
    /// Cloud and only their contiguous source cursor evidence is retained.
    pub fn semantic_from_provider_page(
        page: &a3s_cloud_contracts::AgentProviderEventPageV1,
        projected_at: DateTime<Utc>,
    ) -> Result<Vec<Self>, String> {
        use a3s_cloud_contracts::{AgentProviderRunStateV1, AgentProviderSemanticEventV1};

        page.validate()?;
        let projected_at = canonical_timestamp(projected_at);
        let mut drafts = page
            .events
            .iter()
            .map(|record| match &record.event {
                AgentProviderSemanticEventV1::ModelOutput { text } => Self::new(
                    AgentExecutionEventKind::ModelOutput,
                    AgentEventContent::inline_json(serde_json::json!({"text": text}))?,
                    projected_at,
                ),
                AgentProviderSemanticEventV1::ToolRequest {
                    call_id,
                    tool,
                    request,
                } => Self::new(
                    AgentExecutionEventKind::ToolRequest,
                    AgentEventContent::inline_json(serde_json::json!({
                        "callId": call_id,
                        "tool": tool,
                        "request": request,
                    }))?,
                    projected_at,
                ),
                AgentProviderSemanticEventV1::ToolResult {
                    call_id,
                    tool,
                    request_digest,
                    outcome,
                    result,
                } => Self::new(
                    AgentExecutionEventKind::ToolResult,
                    AgentEventContent::inline_json(serde_json::json!({
                        "callId": call_id,
                        "tool": tool,
                        "requestDigest": request_digest,
                        "outcome": outcome,
                        "result": result,
                    }))?,
                    projected_at,
                ),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !page.state.is_terminal() || page.has_more {
            return Ok(drafts);
        }

        let (kind, content) = match page.state {
            AgentProviderRunStateV1::Completed => (
                AgentExecutionEventKind::ExecutionCompleted,
                serde_json::json!({}),
            ),
            AgentProviderRunStateV1::Failed => (
                AgentExecutionEventKind::ExecutionFailed,
                serde_json::json!({
                    "reason": page
                        .terminal_failure
                        .as_deref()
                        .ok_or_else(|| "failed provider page omitted its reason".to_owned())?
                }),
            ),
            AgentProviderRunStateV1::Cancelled => (
                AgentExecutionEventKind::ExecutionCancelled,
                serde_json::json!({}),
            ),
            _ => return Err("non-terminal provider state reached terminal projection".into()),
        };
        drafts.push(Self::new(
            kind,
            AgentEventContent::inline_json(content)?,
            projected_at,
        )?);
        Ok(drafts)
    }
}

fn normalize_failure_reason(reason: &str) -> String {
    let normalized = reason
        .chars()
        .map(|character| {
            if matches!(character, '\0' | '\r' | '\n') {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let normalized = normalized.trim();
    if normalized.is_empty() {
        return "A3S Code run failed".into();
    }
    if normalized.len() <= super::MAX_AGENT_EXECUTION_FAILURE_BYTES {
        return normalized.into();
    }
    let mut end = super::MAX_AGENT_EXECUTION_FAILURE_BYTES;
    while !normalized.is_char_boundary(end) {
        end -= 1;
    }
    normalized[..end].into()
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
    use a3s_cloud_contracts::{
        AgentProtocolEventPageV1, AgentProtocolRunIdentityV1, AgentProtocolRunStateV1,
        AgentProviderEventPageV1, AgentProviderEventRecordV1, AgentProviderRunIdentityV1,
        AgentProviderRunStateV1, AgentProviderSemanticEventV1, AgentProviderToolPayloadIdentityV1,
        AgentProviderToolResultOutcomeV1, HarnessToolBindingV1, AGENT_PROTOCOL_V1,
    };

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

    #[test]
    fn terminal_code_pages_create_semantic_outcomes_without_source_records() {
        let projected_at = Utc::now();
        for (state, expected_kind, expected_content) in [
            (
                AgentProtocolRunStateV1::Completed,
                AgentExecutionEventKind::ExecutionCompleted,
                serde_json::json!({}),
            ),
            (
                AgentProtocolRunStateV1::Failed,
                AgentExecutionEventKind::ExecutionFailed,
                serde_json::json!({"reason": "A3S Code run failed"}),
            ),
            (
                AgentProtocolRunStateV1::Cancelled,
                AgentExecutionEventKind::ExecutionCancelled,
                serde_json::json!({}),
            ),
        ] {
            let drafts =
                AgentExecutionEventDraft::semantic_from_code_page(&empty_page(state), projected_at)
                    .expect("semantic projection");
            assert_eq!(drafts.len(), 1);
            assert_eq!(drafts[0].kind, expected_kind);
            assert_eq!(drafts[0].content.value(), &expected_content);
        }
    }

    #[test]
    fn code_text_projection_keeps_only_cloud_semantics() {
        let projected_at = Utc::now();
        let page: AgentProtocolEventPageV1 = serde_json::from_value(serde_json::json!({
            "schema": AgentProtocolEventPageV1::SCHEMA,
            "identity": {
                "schema": AgentProtocolRunIdentityV1::SCHEMA,
                "protocol": AGENT_PROTOCOL_V1,
                "agent_release_identity": format!("sha256:{}", "a".repeat(64)),
                "session_id": "session-1",
                "run_id": "run-1"
            },
            "after_event_sequence": null,
            "first_available_sequence": 0,
            "latest_sequence_exclusive": 2,
            "next_after_event_sequence": 1,
            "state": "completed",
            "observed_at_ms": 2,
            "retention_gap": false,
            "has_more": false,
            "events": [
                {
                    "sequence": 0,
                    "occurred_at_ms": 1,
                    "event": {
                        "version": 1,
                        "type": "text_delta",
                        "payload": {"text": "hello", "codeOnly": "discarded"},
                        "metadata": {
                            "session_id": "session-1",
                            "run_id": "run-1",
                            "sequence": 0,
                            "timestamp_ms": 1,
                            "codeOnly": "discarded"
                        }
                    }
                },
                {
                    "sequence": 1,
                    "occurred_at_ms": 2,
                    "event": {
                        "version": 1,
                        "type": "agent_end",
                        "payload": {"codeOnly": "discarded"},
                        "metadata": {
                            "session_id": "session-1",
                            "run_id": "run-1",
                            "sequence": 1,
                            "timestamp_ms": 2
                        }
                    }
                }
            ]
        }))
        .expect("Code page");

        let drafts = AgentExecutionEventDraft::semantic_from_code_page(&page, projected_at)
            .expect("semantic projection");
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].kind, AgentExecutionEventKind::ModelOutput);
        assert_eq!(
            drafts[0].content.value(),
            &serde_json::json!({"text": "hello"})
        );
        assert_eq!(drafts[1].kind, AgentExecutionEventKind::ExecutionCompleted);
        assert_eq!(drafts[1].content.value(), &serde_json::json!({}));
    }

    #[test]
    fn provider_tool_projection_keeps_only_binding_and_payload_identity() {
        let projected_at = Utc::now();
        let tool = HarnessToolBindingV1 {
            name: "workspace.search".into(),
            revision: "1.0.0".into(),
            contract_digest: format!("sha256:{}", "b".repeat(64)),
            approval_required: false,
        };
        let request = AgentProviderToolPayloadIdentityV1 {
            digest: format!("sha256:{}", "c".repeat(64)),
            size_bytes: 128,
            media_type: "application/json".into(),
        };
        let result = AgentProviderToolPayloadIdentityV1 {
            digest: format!("sha256:{}", "d".repeat(64)),
            size_bytes: 256,
            media_type: "application/json".into(),
        };
        let page = AgentProviderEventPageV1 {
            schema: AgentProviderEventPageV1::SCHEMA.into(),
            identity: AgentProviderRunIdentityV1::new(
                format!("sha256:{}", "e".repeat(64)),
                format!("sha256:{}", "f".repeat(64)),
                format!("sha256:{}", "a".repeat(64)),
                "session-1".into(),
                "run-1".into(),
            )
            .expect("provider identity"),
            after_event_sequence: None,
            first_available_sequence: Some(0),
            source_first_sequence: Some(0),
            source_last_sequence: Some(1),
            source_event_count: 2,
            latest_sequence_exclusive: 2,
            next_after_event_sequence: Some(1),
            state: AgentProviderRunStateV1::Executing,
            observed_at_ms: 3,
            retention_gap: false,
            has_more: false,
            terminal_failure: None,
            events: vec![
                AgentProviderEventRecordV1 {
                    sequence: 0,
                    occurred_at_ms: 1,
                    event: AgentProviderSemanticEventV1::ToolRequest {
                        call_id: "call-1".into(),
                        tool: tool.clone(),
                        request: request.clone(),
                    },
                },
                AgentProviderEventRecordV1 {
                    sequence: 1,
                    occurred_at_ms: 2,
                    event: AgentProviderSemanticEventV1::ToolResult {
                        call_id: "call-1".into(),
                        tool: tool.clone(),
                        request_digest: request.digest.clone(),
                        outcome: AgentProviderToolResultOutcomeV1::Succeeded,
                        result: result.clone(),
                    },
                },
            ],
        };

        let drafts = AgentExecutionEventDraft::semantic_from_provider_page(&page, projected_at)
            .expect("Tool semantic projection");
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].kind, AgentExecutionEventKind::ToolRequest);
        assert_eq!(
            drafts[0].content.value(),
            &serde_json::json!({
                "callId": "call-1",
                "tool": tool,
                "request": request,
            })
        );
        assert_eq!(drafts[1].kind, AgentExecutionEventKind::ToolResult);
        assert_eq!(
            drafts[1].content.value(),
            &serde_json::json!({
                "callId": "call-1",
                "tool": tool,
                "requestDigest": format!("sha256:{}", "c".repeat(64)),
                "outcome": "succeeded",
                "result": result,
            })
        );
    }

    fn empty_page(state: AgentProtocolRunStateV1) -> AgentProtocolEventPageV1 {
        AgentProtocolEventPageV1 {
            schema: AgentProtocolEventPageV1::SCHEMA.into(),
            identity: AgentProtocolRunIdentityV1 {
                schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
                protocol: AGENT_PROTOCOL_V1.into(),
                agent_release_identity: format!("sha256:{}", "a".repeat(64)),
                session_id: "session-1".into(),
                run_id: "run-1".into(),
            },
            after_event_sequence: None,
            first_available_sequence: None,
            latest_sequence_exclusive: 0,
            next_after_event_sequence: None,
            state,
            observed_at_ms: 1,
            retention_gap: false,
            has_more: false,
            events: Vec::new(),
        }
    }
}
