use super::metadata;
use crate::{
    AgentProtocolCommandV1, AgentProviderCommandReceiptV1, AgentProviderCommandV1,
    AgentProviderEventPageV1, AgentProviderEventReceiptV1, AgentProviderProfile,
    AgentProviderRunIdentityV1, AgentProviderRunStartV1, AgentProviderRunStateV1,
    NodeAgentProviderEventBatchV1, NodeAgentProviderEventReceiptV1,
    NodeAgentProviderRuntimeBindingV1, NodeCommandAck, NodeCommandEnvelope, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult,
};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

const CODE_PROFILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/a1.3/a3s-code-provider-profile.acl"
));
const REFERENCE_PROFILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/a1.3/reference-echo-provider-profile.acl"
));
const RELEASE_IDENTITY: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RUNTIME_SPEC_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn binding(execution_id: Uuid) -> NodeAgentProviderRuntimeBindingV1 {
    let profile = AgentProviderProfile::parse_acl(CODE_PROFILE).expect("Code provider profile");
    let provider_run_identity = AgentProviderRunIdentityV1::new(
        profile.digest().into(),
        profile.capability_digest().into(),
        RELEASE_IDENTITY.into(),
        "conversation-1".into(),
        "execution-1-attempt-1".into(),
    )
    .expect("provider run identity");
    NodeAgentProviderRuntimeBindingV1 {
        schema: NodeAgentProviderRuntimeBindingV1::SCHEMA.into(),
        execution_id,
        workload_id: Uuid::now_v7(),
        workload_revision_id: Uuid::now_v7(),
        deployment_id: Uuid::now_v7(),
        replica_id: Uuid::now_v7(),
        runtime_unit_id: "workload:agent:revision:7".into(),
        runtime_generation: 7,
        runtime_spec_digest: RUNTIME_SPEC_DIGEST.into(),
        service_port_name: "agent".into(),
        provider_profile_acl: CODE_PROFILE.into(),
        provider_profile_digest: profile.digest().into(),
        provider_run_identity,
    }
}

fn command(binding: &NodeAgentProviderRuntimeBindingV1) -> AgentProviderCommandV1 {
    AgentProviderCommandV1::Start {
        request: AgentProviderRunStartV1::new(
            "execution-1-start".into(),
            binding.provider_run_identity.clone(),
            "Fix the failing test.".into(),
        )
        .expect("provider start request"),
    }
}

#[test]
fn node_command_preserves_the_provider_contract_and_native_code_adapter() {
    let execution_id = Uuid::now_v7();
    let binding = binding(execution_id);
    let provider_command = command(&binding);
    let native = binding
        .code_command(&provider_command)
        .expect("native Code adapter command");
    let AgentProtocolCommandV1::Start { request } = &native else {
        panic!("provider start must remain a native Code start");
    };
    assert_eq!(request.prompt, "Fix the failing test.");
    assert_eq!(
        request.identity.run_id,
        binding.provider_run_identity.run_id
    );

    let mut command_metadata = metadata(1);
    command_metadata.aggregate_id = execution_id;
    let issued_at = command_metadata.issued_at;
    let envelope = NodeCommandEnvelope::new(
        command_metadata,
        NodeCommandPayload::AgentProviderCommand {
            binding: Box::new(binding.clone()),
            command: Box::new(provider_command.clone()),
        },
    )
    .expect("provider-neutral Node command");
    assert_eq!(envelope.payload.kind(), "agent_provider_command");

    let observed_at_ms =
        u64::try_from(issued_at.timestamp_millis() + 1).expect("positive timestamp");
    let receipt = AgentProviderCommandReceiptV1::accepted(
        &binding.profile().expect("bound profile"),
        &provider_command,
        AgentProviderRunStateV1::Created,
        observed_at_ms,
        false,
    )
    .expect("provider receipt");
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
            .expect("completion timestamp"),
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::AgentProviderCommandAccepted {
                receipt: Box::new(receipt),
            }),
        },
    };
    acknowledgement
        .validate_against(&envelope)
        .expect("provider acknowledgement binds its exact command");
}

#[test]
fn node_binding_rejects_mixed_provider_profiles_and_run_identities() {
    let execution_id = Uuid::now_v7();
    let binding = binding(execution_id);
    let provider_command = command(&binding);

    let mut mixed_profile = binding.clone();
    mixed_profile.provider_profile_acl = REFERENCE_PROFILE.into();
    assert!(mixed_profile.validate_command(&provider_command).is_err());

    let mut mixed_identity = binding;
    mixed_identity.provider_run_identity.run_id = "different-run".into();
    assert!(mixed_identity.validate_command(&provider_command).is_err());
}

#[test]
fn provider_event_delivery_binds_profile_cursor_and_receipt_time() {
    let execution_id = Uuid::now_v7();
    let binding = binding(execution_id);
    let page = AgentProviderEventPageV1 {
        schema: AgentProviderEventPageV1::SCHEMA.into(),
        identity: binding.provider_run_identity.clone(),
        after_event_sequence: None,
        first_available_sequence: None,
        source_first_sequence: None,
        source_last_sequence: None,
        source_event_count: 0,
        latest_sequence_exclusive: 0,
        next_after_event_sequence: None,
        state: AgentProviderRunStateV1::Created,
        observed_at_ms: 1,
        retention_gap: false,
        has_more: false,
        terminal_failure: None,
        events: Vec::new(),
    };
    let batch = NodeAgentProviderEventBatchV1 {
        schema: NodeAgentProviderEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: Uuid::now_v7(),
        binding,
        page,
        sent_at_ms: 2,
    };
    batch.validate().expect("provider event batch");
    let receipt = AgentProviderEventReceiptV1::accepted(
        &batch.binding.profile().expect("bound profile"),
        batch.batch_id,
        &batch.page,
        3,
        false,
    )
    .expect("provider event receipt");
    let mut node_receipt = NodeAgentProviderEventReceiptV1 {
        schema: NodeAgentProviderEventReceiptV1::SCHEMA.into(),
        batch_id: batch.batch_id,
        node_id: batch.node_id,
        execution_id,
        receipt,
    };
    node_receipt
        .validate_for(&batch)
        .expect("node provider receipt");
    node_receipt.receipt.accepted_at_ms = 1;
    assert!(node_receipt.validate_for(&batch).is_err());
}
