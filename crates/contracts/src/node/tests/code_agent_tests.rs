use super::metadata;
use crate::{
    AgentProtocolChangeSetV1, AgentProtocolCommandActionV1, AgentProtocolCommandReceiptV1,
    AgentProtocolCommandV1, AgentProtocolEventPageV1, AgentProtocolRunIdentityV1,
    AgentProtocolRunStartV1, AgentProtocolRunStateV1, NodeCodeAgentEventBatchV1,
    NodeCodeAgentEventReceiptV1, NodeCodeAgentRuntimeBindingV1, NodeCommandAck,
    NodeCommandEnvelope, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
    AGENT_PROTOCOL_CHANGE_SET_ENCODING_V1, AGENT_PROTOCOL_CHANGE_SET_FORMAT_V1, AGENT_PROTOCOL_V1,
};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

const RELEASE_IDENTITY: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RUNTIME_SPEC_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn binding(execution_id: Uuid) -> NodeCodeAgentRuntimeBindingV1 {
    NodeCodeAgentRuntimeBindingV1 {
        schema: NodeCodeAgentRuntimeBindingV1::SCHEMA.into(),
        execution_id,
        workload_id: Uuid::now_v7(),
        workload_revision_id: Uuid::now_v7(),
        deployment_id: Uuid::now_v7(),
        replica_id: Uuid::now_v7(),
        runtime_unit_id: "workload:agent:revision:7".into(),
        runtime_generation: 7,
        runtime_spec_digest: RUNTIME_SPEC_DIGEST.into(),
        service_port_name: "agent".into(),
        code_run_identity: identity(),
    }
}

fn identity() -> AgentProtocolRunIdentityV1 {
    AgentProtocolRunIdentityV1 {
        schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
        protocol: AGENT_PROTOCOL_V1.into(),
        agent_release_identity: RELEASE_IDENTITY.into(),
        session_id: "conversation-1".into(),
        run_id: "execution-1-attempt-1".into(),
    }
}

fn command() -> AgentProtocolCommandV1 {
    AgentProtocolCommandV1::Start {
        request: AgentProtocolRunStartV1 {
            schema: AgentProtocolRunStartV1::SCHEMA.into(),
            request_id: "execution-1:start".into(),
            identity: identity(),
            prompt: "Fix the failing test.".into(),
        },
    }
}

#[test]
fn node_commands_wrap_the_exact_code_contract_and_runtime_binding() {
    let execution_id = Uuid::now_v7();
    let binding = binding(execution_id);
    let code_command = command();
    let mut command_metadata = metadata(1);
    command_metadata.aggregate_id = execution_id;
    let issued_at = command_metadata.issued_at;
    let envelope = NodeCommandEnvelope::new(
        command_metadata,
        NodeCommandPayload::CodeAgentCommand {
            binding: Box::new(binding.clone()),
            command: Box::new(code_command.clone()),
        },
    )
    .expect("valid Code Agent node command");

    assert_eq!(envelope.payload.kind(), "code_agent_command");
    assert_eq!(envelope.generation, binding.runtime_generation);
    let observed_at_ms =
        u64::try_from(issued_at.timestamp_millis() + 1).expect("positive command timestamp");
    let code_receipt = AgentProtocolCommandReceiptV1 {
        schema: AgentProtocolCommandReceiptV1::SCHEMA.into(),
        action: AgentProtocolCommandActionV1::Start,
        request_id: code_command.request_id().into(),
        identity: code_command.identity().clone(),
        command_digest: code_command.digest().expect("Code command digest"),
        state: AgentProtocolRunStateV1::Created,
        latest_event_sequence_exclusive: 0,
        observed_at_ms,
        replayed: false,
    };
    let acknowledgement = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: envelope.command_id,
        lease_id: envelope.lease_id,
        node_id: envelope.node_id,
        sequence: envelope.sequence,
        payload_digest: envelope.payload_digest.clone(),
        completed_at: Utc
            .timestamp_millis_opt(i64::try_from(observed_at_ms).expect("timestamp"))
            .single()
            .expect("valid completion timestamp"),
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::CodeAgentCommandAccepted {
                receipt: Box::new(code_receipt),
            }),
        },
    };
    acknowledgement
        .validate_against(&envelope)
        .expect("node receipt must bind the exact Code receipt");

    let mut mismatched = binding;
    mismatched.code_run_identity.run_id = "another-run".into();
    assert!(mismatched.validate_command(&code_command).is_err());
}

#[test]
fn event_delivery_carries_an_unmodified_code_page_and_exact_receipt() {
    let execution_id = Uuid::now_v7();
    let observed_at_ms = 1_723_000_000_000;
    let page = AgentProtocolEventPageV1 {
        schema: AgentProtocolEventPageV1::SCHEMA.into(),
        identity: identity(),
        after_event_sequence: None,
        first_available_sequence: None,
        latest_sequence_exclusive: 0,
        next_after_event_sequence: None,
        state: AgentProtocolRunStateV1::Created,
        observed_at_ms,
        retention_gap: false,
        has_more: false,
        events: Vec::new(),
    };
    let batch = NodeCodeAgentEventBatchV1 {
        schema: NodeCodeAgentEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: Uuid::now_v7(),
        binding: binding(execution_id),
        page,
        change_set: None,
        sent_at_ms: observed_at_ms + 1,
    };
    batch.validate().expect("valid Code event delivery");
    let receipt = NodeCodeAgentEventReceiptV1 {
        schema: NodeCodeAgentEventReceiptV1::SCHEMA.into(),
        batch_id: batch.batch_id,
        node_id: batch.node_id,
        execution_id,
        identity: batch.page.identity.clone(),
        page_digest: batch.page.digest().expect("Code page digest"),
        accepted_after_event_sequence: batch.page.next_after_event_sequence,
        accepted_state: batch.page.state,
        accepted_events: 0,
        accepted_at_ms: batch.sent_at_ms + 1,
        replayed: false,
    };
    receipt
        .validate_for(&batch)
        .expect("exact Code event receipt");

    let mut changed = receipt;
    changed.accepted_at_ms += 1;
    changed.page_digest = format!("sha256:{}", "d".repeat(64));
    assert!(changed.validate_for(&batch).is_err());
}

#[test]
fn change_set_is_only_valid_on_its_exact_terminal_page() {
    let execution_id = Uuid::now_v7();
    let observed_at_ms = 1_723_000_000_000;
    let change_set = AgentProtocolChangeSetV1 {
        schema: AgentProtocolChangeSetV1::SCHEMA.into(),
        identity: identity(),
        state: AgentProtocolRunStateV1::Completed,
        format: AGENT_PROTOCOL_CHANGE_SET_FORMAT_V1.into(),
        encoding: AGENT_PROTOCOL_CHANGE_SET_ENCODING_V1.into(),
        base_tree: format!("git-tree:{}", "a".repeat(40)),
        result_tree: format!("git-tree:{}", "b".repeat(40)),
        patch_digest: "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            .into(),
        patch_bytes: 0,
        patch_base64: String::new(),
        observed_at_ms,
    };
    let page = AgentProtocolEventPageV1 {
        schema: AgentProtocolEventPageV1::SCHEMA.into(),
        identity: identity(),
        after_event_sequence: None,
        first_available_sequence: None,
        latest_sequence_exclusive: 0,
        next_after_event_sequence: None,
        state: AgentProtocolRunStateV1::Completed,
        observed_at_ms,
        retention_gap: false,
        has_more: false,
        events: Vec::new(),
    };
    let batch = NodeCodeAgentEventBatchV1 {
        schema: NodeCodeAgentEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: Uuid::now_v7(),
        binding: binding(execution_id),
        page,
        change_set: Some(change_set),
        sent_at_ms: observed_at_ms + 1,
    };
    batch.validate().expect("terminal change set delivery");

    let mut nonterminal = batch.clone();
    nonterminal.page.state = AgentProtocolRunStateV1::Executing;
    assert!(nonterminal.validate().is_err());

    let mut mismatched = batch;
    mismatched
        .change_set
        .as_mut()
        .expect("change set")
        .identity
        .run_id = "another-run".into();
    assert!(mismatched.validate().is_err());
}
