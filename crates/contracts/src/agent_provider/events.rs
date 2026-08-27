use super::{
    AgentProviderCapabilityV1, AgentProviderProfile, AgentProviderRunIdentityV1,
    AgentProviderRunStateV1, HarnessToolBindingV1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const AGENT_PROVIDER_MAX_EVENTS_PER_PAGE: usize = 64;
pub const AGENT_PROVIDER_MAX_EVENT_TEXT_BYTES: usize = 64 * 1024;
pub const AGENT_PROVIDER_MAX_FAILURE_BYTES: usize = 16 * 1024;
pub const AGENT_PROVIDER_EVENT_PAGE_HTTP_PATH_V1: &str = "/v1/agent-provider/events/page";
pub const AGENT_PROVIDER_MAX_EVENT_PAGE_BYTES: usize = 5 * 1024 * 1024;
pub const AGENT_PROVIDER_MAX_TOOL_PAYLOAD_BYTES: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProviderEventPageRequestV1 {
    pub schema: String,
    pub identity: AgentProviderRunIdentityV1,
    pub after_event_sequence: Option<u64>,
    pub limit: u16,
}

impl AgentProviderEventPageRequestV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.agent-provider-event-page-request.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Agent provider event-page request schema {:?}",
                self.schema
            ));
        }
        self.identity.validate()?;
        if self.limit == 0 || usize::from(self.limit) > AGENT_PROVIDER_MAX_EVENTS_PER_PAGE {
            return Err("Agent provider event-page request limit is invalid".into());
        }
        Ok(())
    }

    pub fn validate_for(&self, profile: &AgentProviderProfile) -> Result<(), String> {
        self.validate()?;
        self.identity.validate_for(profile)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentProviderSemanticEventV1 {
    ModelOutput {
        text: String,
    },
    ToolRequest {
        call_id: String,
        tool: HarnessToolBindingV1,
        request: AgentProviderToolPayloadIdentityV1,
    },
    ToolResult {
        call_id: String,
        tool: HarnessToolBindingV1,
        request_digest: String,
        outcome: AgentProviderToolResultOutcomeV1,
        result: AgentProviderToolPayloadIdentityV1,
    },
}

impl AgentProviderSemanticEventV1 {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::ModelOutput { text } => validate_bounded_content(
                "Agent provider model output",
                text,
                AGENT_PROVIDER_MAX_EVENT_TEXT_BYTES,
            ),
            Self::ToolRequest {
                call_id,
                tool,
                request,
            } => {
                validate_single_line("Agent provider Tool call ID", call_id, 256)?;
                tool.validate()?;
                request.validate()
            }
            Self::ToolResult {
                call_id,
                tool,
                request_digest,
                result,
                ..
            } => {
                validate_single_line("Agent provider Tool call ID", call_id, 256)?;
                tool.validate()?;
                validate_digest("Agent provider Tool request digest", request_digest)?;
                result.validate()
            }
        }
    }

    pub const fn is_tool_event(&self) -> bool {
        matches!(self, Self::ToolRequest { .. } | Self::ToolResult { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProviderToolResultOutcomeV1 {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProviderToolPayloadIdentityV1 {
    pub digest: String,
    pub size_bytes: u64,
    pub media_type: String,
}

impl AgentProviderToolPayloadIdentityV1 {
    pub fn validate(&self) -> Result<(), String> {
        validate_digest("Agent provider Tool payload digest", &self.digest)?;
        if self.size_bytes > AGENT_PROVIDER_MAX_TOOL_PAYLOAD_BYTES {
            return Err("Agent provider Tool payload size is invalid".into());
        }
        validate_single_line(
            "Agent provider Tool payload media type",
            &self.media_type,
            255,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProviderEventRecordV1 {
    pub sequence: u64,
    pub occurred_at_ms: u64,
    pub event: AgentProviderSemanticEventV1,
}

impl AgentProviderEventRecordV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.occurred_at_ms == 0 {
            return Err("Agent provider event time must be positive".into());
        }
        self.event.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProviderEventPageV1 {
    pub schema: String,
    pub identity: AgentProviderRunIdentityV1,
    pub after_event_sequence: Option<u64>,
    pub first_available_sequence: Option<u64>,
    pub source_first_sequence: Option<u64>,
    pub source_last_sequence: Option<u64>,
    pub source_event_count: u16,
    pub latest_sequence_exclusive: u64,
    pub next_after_event_sequence: Option<u64>,
    pub state: AgentProviderRunStateV1,
    pub observed_at_ms: u64,
    pub retention_gap: bool,
    pub has_more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_failure: Option<String>,
    pub events: Vec<AgentProviderEventRecordV1>,
}

impl AgentProviderEventPageV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.agent-provider-event-page.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Agent provider event-page schema {:?}",
                self.schema
            ));
        }
        self.identity.validate()?;
        if self.observed_at_ms == 0
            || usize::from(self.source_event_count) > AGENT_PROVIDER_MAX_EVENTS_PER_PAGE
            || self.events.len() > usize::from(self.source_event_count)
        {
            return Err("Agent provider event-page bounds are invalid".into());
        }
        if self.retention_gap {
            return self.validate_retention_gap();
        }
        self.validate_terminal_failure()?;
        self.validate_contiguous_page()
    }

    pub fn validate_for(&self, profile: &AgentProviderProfile) -> Result<(), String> {
        self.validate()?;
        self.identity.validate_for(profile)?;
        if self
            .events
            .iter()
            .any(|record| record.event.is_tool_event())
            && !profile.supports(AgentProviderCapabilityV1::ToolCalls)
        {
            return Err(
                "Agent provider emitted Tool events without the tool_calls capability".into(),
            );
        }
        let approval_requests = self
            .events
            .iter()
            .filter(|record| {
                matches!(
                    &record.event,
                    AgentProviderSemanticEventV1::ToolRequest { tool, .. }
                        if tool.approval_required
                )
            })
            .collect::<Vec<_>>();
        if self.state == AgentProviderRunStateV1::AwaitingApproval
            && !profile.supports(AgentProviderCapabilityV1::PauseResume)
        {
            return Err(
                "Agent provider paused for approval without the pause_resume capability".into(),
            );
        }
        if let [request] = approval_requests.as_slice() {
            if !profile.supports(AgentProviderCapabilityV1::PauseResume)
                || self.state != AgentProviderRunStateV1::AwaitingApproval
                || self.has_more
                || self.retention_gap
                || self.source_last_sequence != Some(request.sequence)
            {
                return Err(
                    "approval-required Tool request did not close one paused provider page".into(),
                );
            }
        } else if !approval_requests.is_empty() {
            return Err("Agent provider page opened multiple approval checkpoints".into());
        } else if self.state == AgentProviderRunStateV1::AwaitingApproval
            && (self.source_event_count != 0 || !self.events.is_empty())
        {
            return Err(
                "paused Agent provider emitted events without its approval checkpoint".into(),
            );
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)
            .map_err(|error| format!("could not encode Agent provider event page: {error}"))?;
        Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
    }

    fn validate_retention_gap(&self) -> Result<(), String> {
        let expected = next_sequence(self.after_event_sequence)?;
        if !self.events.is_empty()
            || self.terminal_failure.is_some()
            || self.source_first_sequence.is_some()
            || self.source_last_sequence.is_some()
            || self.source_event_count != 0
            || self.has_more
            || self.next_after_event_sequence != self.after_event_sequence
            || self
                .first_available_sequence
                .is_none_or(|first| first <= expected || first > self.latest_sequence_exclusive)
        {
            return Err("Agent provider retention-gap evidence is invalid".into());
        }
        Ok(())
    }

    fn validate_contiguous_page(&self) -> Result<(), String> {
        let expected_first = next_sequence(self.after_event_sequence)?;
        if self
            .first_available_sequence
            .is_some_and(|first| first > expected_first)
        {
            return Err("Agent provider event page omitted required retention-gap evidence".into());
        }
        let expected_cursor = if self.source_event_count == 0 {
            if self.source_first_sequence.is_some() || self.source_last_sequence.is_some() {
                return Err("empty Agent provider page carries source sequence evidence".into());
            }
            self.after_event_sequence
        } else {
            let source_first = self.source_first_sequence.ok_or_else(|| {
                "Agent provider page omitted its first source sequence".to_owned()
            })?;
            let source_last = self
                .source_last_sequence
                .ok_or_else(|| "Agent provider page omitted its last source sequence".to_owned())?;
            let source_span = source_last
                .checked_sub(source_first)
                .and_then(|span| span.checked_add(1))
                .ok_or_else(|| "Agent provider source event sequence regressed".to_owned())?;
            if source_first != expected_first || source_span != u64::from(self.source_event_count) {
                return Err(
                    "Agent provider source page is not a contiguous bounded sequence".into(),
                );
            }
            Some(source_last)
        };
        let mut previous = self.after_event_sequence;
        for record in &self.events {
            record.validate()?;
            if previous.is_some_and(|sequence| record.sequence <= sequence)
                || self
                    .source_first_sequence
                    .is_none_or(|first| record.sequence < first)
                || self
                    .source_last_sequence
                    .is_none_or(|last| record.sequence > last)
                || record.occurred_at_ms > self.observed_at_ms
            {
                return Err(
                    "Agent provider semantic events are not an ordered source subset".into(),
                );
            }
            previous = Some(record.sequence);
        }
        if self.next_after_event_sequence != expected_cursor
            || expected_cursor.is_some_and(|cursor| cursor >= self.latest_sequence_exclusive)
            || (self.has_more
                && expected_cursor.is_none_or(|cursor| {
                    cursor
                        .checked_add(1)
                        .is_none_or(|next| next >= self.latest_sequence_exclusive)
                }))
            || (!self.has_more
                && match expected_cursor {
                    Some(cursor) => cursor.checked_add(1) != Some(self.latest_sequence_exclusive),
                    None => self.latest_sequence_exclusive != 0,
                })
        {
            return Err("Agent provider event-page cursor evidence is invalid".into());
        }
        Ok(())
    }

    fn validate_terminal_failure(&self) -> Result<(), String> {
        match (
            &self.terminal_failure,
            self.state,
            self.has_more,
            self.retention_gap,
        ) {
            (Some(reason), AgentProviderRunStateV1::Failed, false, false) => {
                validate_bounded_content(
                    "Agent provider terminal failure",
                    reason,
                    AGENT_PROVIDER_MAX_FAILURE_BYTES,
                )?;
                if reason.contains(['\r', '\n']) {
                    return Err("Agent provider terminal failure must be single-line".into());
                }
                Ok(())
            }
            (None, AgentProviderRunStateV1::Failed, false, false) => {
                Err("failed Agent provider terminal page omitted its bounded reason".into())
            }
            (None, _, _, _) => Ok(()),
            (Some(_), _, _, _) => {
                Err("Agent provider failure reason is not bound to a terminal failed page".into())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProviderEventReceiptV1 {
    pub schema: String,
    pub batch_id: uuid::Uuid,
    pub identity: AgentProviderRunIdentityV1,
    pub page_digest: String,
    pub accepted_after_event_sequence: Option<u64>,
    pub accepted_state: AgentProviderRunStateV1,
    pub accepted_events: u16,
    pub accepted_source_events: u16,
    pub accepted_at_ms: u64,
    pub replayed: bool,
}

impl AgentProviderEventReceiptV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.agent-provider-event-receipt.v1";

    pub fn accepted(
        profile: &AgentProviderProfile,
        batch_id: uuid::Uuid,
        page: &AgentProviderEventPageV1,
        accepted_at_ms: u64,
        replayed: bool,
    ) -> Result<Self, String> {
        page.validate_for(profile)?;
        let receipt = Self {
            schema: Self::SCHEMA.into(),
            batch_id,
            identity: page.identity.clone(),
            page_digest: page.digest()?,
            accepted_after_event_sequence: page.next_after_event_sequence,
            accepted_state: page.state,
            accepted_events: u16::try_from(page.events.len())
                .map_err(|_| "Agent provider event count exceeds receipt bounds".to_owned())?,
            accepted_source_events: page.source_event_count,
            accepted_at_ms,
            replayed,
        };
        receipt.validate_for(profile, batch_id, page)?;
        Ok(receipt)
    }

    pub fn validate_for(
        &self,
        profile: &AgentProviderProfile,
        batch_id: uuid::Uuid,
        page: &AgentProviderEventPageV1,
    ) -> Result<(), String> {
        page.validate_for(profile)?;
        if self.schema != Self::SCHEMA
            || self.batch_id.is_nil()
            || self.batch_id != batch_id
            || self.identity != page.identity
            || self.page_digest != page.digest()?
            || self.accepted_after_event_sequence != page.next_after_event_sequence
            || self.accepted_state != page.state
            || usize::from(self.accepted_events) != page.events.len()
            || self.accepted_source_events != page.source_event_count
            || self.accepted_at_ms == 0
        {
            return Err("Agent provider event receipt changed its pending page identity".into());
        }
        Ok(())
    }
}

fn next_sequence(after: Option<u64>) -> Result<u64, String> {
    after
        .map(|value| {
            value
                .checked_add(1)
                .ok_or_else(|| "Agent provider event cursor overflowed".to_owned())
        })
        .unwrap_or(Ok(0))
}

fn validate_bounded_content(label: &str, value: &str, max: usize) -> Result<(), String> {
    if value.is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{label} bounds are invalid"))
    } else {
        Ok(())
    }
}

fn validate_single_line(label: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
        Err(format!(
            "{label} must be a bounded nonempty single-line value"
        ))
    } else {
        Ok(())
    }
}

fn validate_digest(label: &str, value: &str) -> Result<(), String> {
    let valid = value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if valid {
        Ok(())
    } else {
        Err(format!(
            "{label} must use canonical lowercase SHA-256 syntax"
        ))
    }
}
