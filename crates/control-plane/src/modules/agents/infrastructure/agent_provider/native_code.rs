use crate::modules::agents::domain::{
    AgentCodeRunBinding, AgentExecutionProvider, AgentProviderProfileBinding,
};
use a3s_cloud_contracts::{
    AgentEventTypeV1, AgentProtocolCommandReceiptV1, AgentProtocolCommandV1,
    AgentProtocolEventPageV1, AgentProtocolRunCancelV1, AgentProtocolRunRecoverV1,
    AgentProtocolRunStartV1, AgentProtocolRunStateV1, AgentProviderCommandReceiptV1,
    AgentProviderCommandV1, AgentProviderEventPageV1, AgentProviderEventRecordV1,
    AgentProviderRunStateV1, AgentProviderSemanticEventV1,
};

#[derive(Debug, Clone)]
pub struct NativeCodeAgentExecutionProvider {
    profile: AgentProviderProfileBinding,
}

impl NativeCodeAgentExecutionProvider {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            profile: AgentProviderProfileBinding::native_code()?,
        })
    }
}

impl AgentExecutionProvider for NativeCodeAgentExecutionProvider {
    fn profile(&self) -> &AgentProviderProfileBinding {
        &self.profile
    }
}

pub(crate) fn encode_code_command(
    binding: &AgentCodeRunBinding,
    command: &AgentProviderCommandV1,
) -> Result<AgentProtocolCommandV1, String> {
    binding.validate()?;
    let profile = binding.provider()?.profile()?;
    command.validate_for(&profile)?;
    if command.identity() != &binding.provider_identity()? {
        return Err("Agent provider command changed its bound Code run identity".into());
    }
    let identity = binding.identity().clone();
    let native = match command {
        AgentProviderCommandV1::Start { request } => AgentProtocolCommandV1::Start {
            request: AgentProtocolRunStartV1 {
                schema: AgentProtocolRunStartV1::SCHEMA.into(),
                request_id: request.request_id.clone(),
                identity,
                prompt: request.prompt.clone(),
            },
        },
        AgentProviderCommandV1::Cancel { request } => AgentProtocolCommandV1::Cancel {
            request: AgentProtocolRunCancelV1 {
                schema: AgentProtocolRunCancelV1::SCHEMA.into(),
                request_id: request.request_id.clone(),
                identity,
                reason: request.reason.clone(),
            },
        },
        AgentProviderCommandV1::Recover { request } => AgentProtocolCommandV1::Recover {
            request: AgentProtocolRunRecoverV1 {
                schema: AgentProtocolRunRecoverV1::SCHEMA.into(),
                request_id: request.request_id.clone(),
                identity,
                checkpoint_run_id: request.checkpoint_run_id.clone(),
            },
        },
    };
    native
        .validate()
        .map_err(|error| format!("invalid native A3S Code command ({})", error.code()))?;
    Ok(native)
}

pub(crate) fn accept_code_receipt(
    binding: &AgentCodeRunBinding,
    command: &AgentProviderCommandV1,
    receipt: &AgentProtocolCommandReceiptV1,
) -> Result<AgentProviderCommandReceiptV1, String> {
    let native = encode_code_command(binding, command)?;
    receipt
        .validate_for(&native)
        .map_err(|error| format!("invalid native A3S Code receipt ({})", error.code()))?;
    let state = provider_state(receipt.state);
    let common = AgentProviderCommandReceiptV1::accepted(
        &binding.provider()?.profile()?,
        command,
        state,
        receipt.observed_at_ms,
        receipt.replayed,
    )?;
    common.validate_for(&binding.provider()?.profile()?, command)?;
    Ok(common)
}

pub(crate) fn project_code_event_page(
    binding: &AgentCodeRunBinding,
    page: &AgentProtocolEventPageV1,
) -> Result<AgentProviderEventPageV1, String> {
    binding.validate()?;
    page.validate()
        .map_err(|error| format!("invalid native A3S Code event page ({})", error.code()))?;
    if page.identity != *binding.identity() {
        return Err("native A3S Code page changed its provider run identity".into());
    }
    if page.retention_gap {
        let projected = AgentProviderEventPageV1 {
            schema: AgentProviderEventPageV1::SCHEMA.into(),
            identity: binding.provider_identity()?,
            after_event_sequence: page.after_event_sequence,
            first_available_sequence: page.first_available_sequence,
            source_first_sequence: None,
            source_last_sequence: None,
            source_event_count: 0,
            latest_sequence_exclusive: page.latest_sequence_exclusive,
            next_after_event_sequence: page.after_event_sequence,
            state: provider_state(page.state),
            observed_at_ms: page.observed_at_ms,
            retention_gap: true,
            has_more: false,
            terminal_failure: None,
            events: Vec::new(),
        };
        projected.validate_for(&binding.provider()?.profile()?)?;
        return Ok(projected);
    }

    let events = page
        .events
        .iter()
        .filter_map(|record| {
            (record.event.event_type == AgentEventTypeV1::TEXT_DELTA).then_some(record)
        })
        .map(|record| {
            let text = record
                .event
                .payload
                .get("text")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "A3S Code text_delta omitted its text value".to_owned())?;
            Ok(AgentProviderEventRecordV1 {
                sequence: record.sequence,
                occurred_at_ms: record.occurred_at_ms,
                event: AgentProviderSemanticEventV1::ModelOutput { text: text.into() },
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let terminal_failure = if page.state == AgentProtocolRunStateV1::Failed && !page.has_more {
        Some(
            page.events
                .iter()
                .rev()
                .find(|record| record.event.event_type == AgentEventTypeV1::ERROR)
                .and_then(|record| record.event.payload.get("message"))
                .and_then(serde_json::Value::as_str)
                .map(normalize_failure_reason)
                .unwrap_or_else(|| "A3S Code run failed".into()),
        )
    } else {
        None
    };
    let source_event_count = u16::try_from(page.events.len())
        .map_err(|_| "A3S Code source event count exceeds provider bounds".to_owned())?;
    let projected = AgentProviderEventPageV1 {
        schema: AgentProviderEventPageV1::SCHEMA.into(),
        identity: binding.provider_identity()?,
        after_event_sequence: page.after_event_sequence,
        first_available_sequence: page.first_available_sequence,
        source_first_sequence: page.events.first().map(|record| record.sequence),
        source_last_sequence: page.events.last().map(|record| record.sequence),
        source_event_count,
        latest_sequence_exclusive: page.latest_sequence_exclusive,
        next_after_event_sequence: page.next_after_event_sequence,
        state: provider_state(page.state),
        observed_at_ms: page.observed_at_ms,
        retention_gap: false,
        has_more: page.has_more,
        terminal_failure,
        events,
    };
    projected.validate_for(&binding.provider()?.profile()?)?;
    Ok(projected)
}

fn provider_state(state: AgentProtocolRunStateV1) -> AgentProviderRunStateV1 {
    match state {
        AgentProtocolRunStateV1::Created => AgentProviderRunStateV1::Created,
        AgentProtocolRunStateV1::Planning => AgentProviderRunStateV1::Planning,
        AgentProtocolRunStateV1::Executing => AgentProviderRunStateV1::Executing,
        AgentProtocolRunStateV1::Verifying => AgentProviderRunStateV1::Verifying,
        AgentProtocolRunStateV1::Completed => AgentProviderRunStateV1::Completed,
        AgentProtocolRunStateV1::Failed => AgentProviderRunStateV1::Failed,
        AgentProtocolRunStateV1::Cancelled => AgentProviderRunStateV1::Cancelled,
    }
}

fn normalize_failure_reason(reason: &str) -> String {
    const MAX_FAILURE_BYTES: usize = 16 * 1024;

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
    if normalized.len() <= MAX_FAILURE_BYTES {
        return normalized.into();
    }
    let mut end = MAX_FAILURE_BYTES;
    while !normalized.is_char_boundary(end) {
        end -= 1;
    }
    normalized[..end].into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        DeploymentId, NodeId, Sha256Digest, WorkloadId, WorkloadReplicaId, WorkloadRevisionId,
    };
    use a3s_cloud_contracts::{AgentProtocolRunIdentityV1, AgentProviderRunIdentityV1};
    use chrono::Utc;

    fn binding(provider: &NativeCodeAgentExecutionProvider) -> AgentCodeRunBinding {
        AgentCodeRunBinding::new_with_provider(
            provider.profile().clone(),
            NodeId::new(),
            WorkloadId::new(),
            WorkloadRevisionId::new(),
            DeploymentId::new(),
            WorkloadReplicaId::new(),
            "agent-runtime:revision:1",
            1,
            Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("Runtime digest"),
            "agent",
            AgentProtocolRunIdentityV1 {
                schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
                protocol: a3s_cloud_contracts::AGENT_PROTOCOL_V1.into(),
                agent_release_identity: format!("sha256:{}", "a".repeat(64)),
                session_id: "conversation-1".into(),
                run_id: "execution-1".into(),
            },
            Utc::now(),
        )
        .expect("Code binding")
    }

    #[test]
    fn native_adapter_preserves_the_exact_code_command() {
        let provider = NativeCodeAgentExecutionProvider::new().expect("provider");
        let binding = binding(&provider);
        let command = provider
            .start_command(
                "execution-1-start".into(),
                binding.provider_identity().expect("provider identity"),
                "hello".into(),
            )
            .expect("provider command");
        let native = encode_code_command(&binding, &command).expect("Code command");
        assert_eq!(native.request_id(), "execution-1-start");
        assert_eq!(native.identity(), binding.identity());
        let AgentProviderRunIdentityV1 {
            provider_profile_digest,
            ..
        } = command.identity();
        assert_eq!(provider_profile_digest, provider.profile().profile_digest());
    }

    #[test]
    fn native_event_page_projects_only_bounded_semantics_with_source_evidence() {
        let provider = NativeCodeAgentExecutionProvider::new().expect("provider");
        let binding = binding(&provider);
        let observed_at_ms = u64::try_from(Utc::now().timestamp_millis()).expect("timestamp");
        let event = serde_json::from_value(serde_json::json!({
            "sequence": 0,
            "occurred_at_ms": observed_at_ms,
            "event": {
                "version": 1,
                "type": "text_delta",
                "payload": {"text": "hello"},
                "metadata": {
                    "session_id": binding.identity().session_id.as_str(),
                    "run_id": binding.identity().run_id.as_str(),
                    "sequence": 0,
                    "timestamp_ms": observed_at_ms
                }
            }
        }))
        .expect("native Code event");
        let page = AgentProtocolEventPageV1 {
            schema: AgentProtocolEventPageV1::SCHEMA.into(),
            identity: binding.identity().clone(),
            after_event_sequence: None,
            first_available_sequence: Some(0),
            latest_sequence_exclusive: 1,
            next_after_event_sequence: Some(0),
            state: AgentProtocolRunStateV1::Executing,
            observed_at_ms,
            retention_gap: false,
            has_more: false,
            events: vec![event],
        };
        let projected = project_code_event_page(&binding, &page).expect("provider projection");
        assert_eq!(projected.source_event_count, 1);
        assert_eq!(projected.source_first_sequence, Some(0));
        assert_eq!(projected.source_last_sequence, Some(0));
        assert_eq!(projected.events.len(), 1);
        assert!(matches!(
            &projected.events[0].event,
            AgentProviderSemanticEventV1::ModelOutput { text } if text == "hello"
        ));
    }

    #[test]
    fn native_retention_gap_becomes_provider_recovery_evidence() {
        let provider = NativeCodeAgentExecutionProvider::new().expect("provider");
        let binding = binding(&provider);
        let observed_at_ms = u64::try_from(Utc::now().timestamp_millis()).expect("timestamp");
        let retained = serde_json::from_value(serde_json::json!({
            "sequence": 2,
            "occurred_at_ms": observed_at_ms,
            "event": {
                "version": 1,
                "type": "text_delta",
                "payload": {"text": "retained tail"},
                "metadata": {
                    "session_id": binding.identity().session_id.as_str(),
                    "run_id": binding.identity().run_id.as_str(),
                    "sequence": 2,
                    "timestamp_ms": observed_at_ms
                }
            }
        }))
        .expect("retained native Code event");
        let page = AgentProtocolEventPageV1 {
            schema: AgentProtocolEventPageV1::SCHEMA.into(),
            identity: binding.identity().clone(),
            after_event_sequence: Some(0),
            first_available_sequence: Some(2),
            latest_sequence_exclusive: 3,
            next_after_event_sequence: Some(2),
            state: AgentProtocolRunStateV1::Executing,
            observed_at_ms,
            retention_gap: true,
            has_more: false,
            events: vec![retained],
        };
        let projected = project_code_event_page(&binding, &page).expect("recovery projection");
        assert!(projected.retention_gap);
        assert!(projected.events.is_empty());
        assert_eq!(projected.after_event_sequence, Some(0));
        assert_eq!(projected.first_available_sequence, Some(2));
        assert_eq!(projected.next_after_event_sequence, Some(0));
    }
}
