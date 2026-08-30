use super::tests::{claim_command, CodeHarnessRuntime, RecordingCodeHarness};
use super::CommandExecutor;
use crate::agent_provider_harness::{AgentProviderHarnessError, AgentProviderHarnessTransport};
use crate::code_harness::CodeHarnessTransport;
use crate::FileCommandJournal;
use a3s_cloud_contracts::{
    AgentProviderApprovalDecisionV1, AgentProviderApprovalOutcomeV1, AgentProviderCommandReceiptV1,
    AgentProviderCommandV1, AgentProviderEventPageRequestV1, AgentProviderEventPageV1,
    AgentProviderProfile, AgentProviderRunIdentityV1, AgentProviderRunResumeV1,
    AgentProviderRunStartV1, AgentProviderRunStateV1, HarnessToolBindingV1,
    NodeAgentProviderRuntimeBindingV1, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
};
use a3s_runtime::contract::{
    RuntimeEvidence, RuntimeObservation, RuntimeServiceEndpoint, RuntimeUnitClass, RuntimeUnitState,
};
use async_trait::async_trait;
use chrono::Utc;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

struct RecordingAgentProviderHarness {
    calls: AtomicUsize,
    expected_endpoint: RuntimeServiceEndpoint,
}

#[async_trait]
impl AgentProviderHarnessTransport for RecordingAgentProviderHarness {
    async fn send_command(
        &self,
        endpoint: &RuntimeServiceEndpoint,
        binding: &NodeAgentProviderRuntimeBindingV1,
        command: &AgentProviderCommandV1,
        timeout: std::time::Duration,
    ) -> Result<AgentProviderCommandReceiptV1, AgentProviderHarnessError> {
        assert_eq!(endpoint, &self.expected_endpoint);
        assert!(!timeout.is_zero());
        self.calls.fetch_add(1, Ordering::SeqCst);
        let profile = binding
            .profile()
            .map_err(AgentProviderHarnessError::Protocol)?;
        let state = if matches!(command, AgentProviderCommandV1::Resume { .. }) {
            AgentProviderRunStateV1::Executing
        } else {
            AgentProviderRunStateV1::Created
        };
        AgentProviderCommandReceiptV1::accepted(
            &profile,
            command,
            state,
            u64::try_from(Utc::now().timestamp_millis())
                .map_err(|error| AgentProviderHarnessError::Protocol(error.to_string()))?,
            false,
        )
        .map_err(AgentProviderHarnessError::Protocol)
    }

    async fn event_page(
        &self,
        _endpoint: &RuntimeServiceEndpoint,
        _binding: &NodeAgentProviderRuntimeBindingV1,
        _request: &AgentProviderEventPageRequestV1,
        _timeout: std::time::Duration,
    ) -> Result<AgentProviderEventPageV1, AgentProviderHarnessError> {
        Err(AgentProviderHarnessError::Invalid(
            "unexpected provider event page request".into(),
        ))
    }
}

#[tokio::test]
async fn reference_provider_commands_use_only_the_common_provider_transport() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let profile = AgentProviderProfile::parse_acl(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/a1.3/reference-echo-provider-profile.acl"
    )))
    .expect("reference provider profile");
    let identity = AgentProviderRunIdentityV1::new(
        profile.digest().into(),
        profile.capability_digest().into(),
        format!("sha256:{}", "a".repeat(64)),
        "conversation-reference".into(),
        "execution-reference-attempt-1".into(),
    )
    .expect("provider run identity");
    let binding = NodeAgentProviderRuntimeBindingV1 {
        schema: NodeAgentProviderRuntimeBindingV1::SCHEMA.into(),
        execution_id: Uuid::now_v7(),
        workload_id: Uuid::now_v7(),
        workload_revision_id: Uuid::now_v7(),
        deployment_id: Uuid::now_v7(),
        replica_id: Uuid::now_v7(),
        runtime_unit_id: "reference-agent:revision:1".into(),
        runtime_generation: 1,
        runtime_spec_digest: format!("sha256:{}", "b".repeat(64)),
        service_port_name: "agent".into(),
        provider_profile_acl: profile.canonical_acl().into(),
        provider_profile_digest: profile.digest().into(),
        provider_run_identity: identity.clone(),
    };
    let command = AgentProviderCommandV1::Start {
        request: AgentProviderRunStartV1::new(
            "execution-reference:start".into(),
            identity.clone(),
            "Echo this input.".into(),
        )
        .expect("reference start command"),
    };
    let endpoint = RuntimeServiceEndpoint::node_local_tcp("agent", 49_153)
        .expect("node-local provider endpoint");
    let mut claims = BTreeMap::new();
    endpoint
        .insert_claim(&mut claims)
        .expect("Runtime endpoint claim");
    let now_ms = u64::try_from(Utc::now().timestamp_millis()).expect("current timestamp");
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: binding.runtime_unit_id.clone(),
        generation: binding.runtime_generation,
        spec_digest: binding.runtime_spec_digest.clone(),
        class: RuntimeUnitClass::Service,
        state: RuntimeUnitState::Running,
        provider_resource_id: Some("provider-reference-agent".into()),
        provider_build: Some("a3s-box-test".into()),
        observed_at_ms: now_ms,
        started_at_ms: Some(now_ms.saturating_sub(1)),
        finished_at_ms: None,
        health: None,
        outputs: Vec::new(),
        usage: None,
        evidence: Some(RuntimeEvidence {
            provider_build: "a3s-box-test".into(),
            spec_digest: binding.runtime_spec_digest.clone(),
            semantics_profile_digest: None,
            identity_attachment_digest: None,
            claims,
        }),
        provider_attestation: None,
        failure: None,
    };
    observation.validate().expect("Runtime observation");
    let runtime = Arc::new(CodeHarnessRuntime {
        calls: AtomicUsize::new(0),
        observation,
    });
    let code_harness = Arc::new(RecordingCodeHarness {
        calls: AtomicUsize::new(0),
        expected_endpoint: endpoint.clone(),
    });
    let provider_harness = Arc::new(RecordingAgentProviderHarness {
        calls: AtomicUsize::new(0),
        expected_endpoint: endpoint,
    });
    let code_transport: Arc<dyn CodeHarnessTransport> = code_harness.clone();
    let provider_transport: Arc<dyn AgentProviderHarnessTransport> = provider_harness.clone();
    let executor = CommandExecutor::runtime_only(
        FileCommandJournal::new(directory.path(), node_id).expect("journal"),
        runtime.clone(),
    )
    .with_code_harness(code_transport)
    .with_agent_provider_harness(provider_transport);
    let envelope = claim_command(
        node_id,
        binding.execution_id,
        1,
        binding.runtime_generation,
        NodeCommandPayload::AgentProviderCommand {
            binding: Box::new(binding.clone()),
            command: Box::new(command.clone()),
        },
    );

    let acknowledgement = executor
        .execute(envelope.clone())
        .await
        .expect("dispatch reference provider command");
    acknowledgement
        .validate_against(&envelope)
        .expect("exact provider acknowledgement");
    let NodeCommandOutcome::Succeeded { result } = &acknowledgement.outcome else {
        panic!("reference provider command must succeed");
    };
    let NodeCommandResult::AgentProviderCommandAccepted { receipt } = result.as_ref() else {
        panic!("reference provider command returned another result kind");
    };
    receipt
        .validate_for(&profile, &command)
        .expect("exact common provider receipt");
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider_harness.calls.load(Ordering::SeqCst), 1);
    assert_eq!(code_harness.calls.load(Ordering::SeqCst), 0);

    let decision = AgentProviderApprovalDecisionV1::new(
        Uuid::now_v7().to_string(),
        Uuid::now_v7().to_string(),
        &identity,
        "publish-1".into(),
        HarnessToolBindingV1 {
            name: "workspace.publish".into(),
            revision: "1.0.0".into(),
            contract_digest: format!("sha256:{}", "d".repeat(64)),
            approval_required: true,
        },
        format!("sha256:{}", "e".repeat(64)),
        AgentProviderApprovalOutcomeV1::Approved,
        now_ms,
    )
    .expect("approval decision");
    let resume_command = AgentProviderCommandV1::Resume {
        request: AgentProviderRunResumeV1::new(
            "execution-reference:resume:publish-1".into(),
            identity.clone(),
            decision,
        )
        .expect("reference resume command"),
    };
    let resume_envelope = claim_command(
        node_id,
        binding.execution_id,
        2,
        binding.runtime_generation,
        NodeCommandPayload::AgentProviderCommand {
            binding: Box::new(binding.clone()),
            command: Box::new(resume_command.clone()),
        },
    );
    let resumed = executor
        .execute(resume_envelope.clone())
        .await
        .expect("dispatch reference provider resume");
    resumed
        .validate_against(&resume_envelope)
        .expect("exact resume acknowledgement");
    let NodeCommandOutcome::Succeeded { result } = &resumed.outcome else {
        panic!("reference provider resume must succeed");
    };
    let NodeCommandResult::AgentProviderCommandAccepted { receipt } = result.as_ref() else {
        panic!("reference provider resume returned another result kind");
    };
    receipt
        .validate_for(&profile, &resume_command)
        .expect("exact common provider resume receipt");
    assert_eq!(receipt.state, AgentProviderRunStateV1::Executing);
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 2);
    assert_eq!(provider_harness.calls.load(Ordering::SeqCst), 2);
    assert_eq!(code_harness.calls.load(Ordering::SeqCst), 0);

    let mut redelivery = envelope;
    redelivery.lease_id = Uuid::now_v7();
    let replay = executor
        .execute(redelivery)
        .await
        .expect("replay reference provider command");
    assert_eq!(replay.outcome, acknowledgement.outcome);
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 2);
    assert_eq!(provider_harness.calls.load(Ordering::SeqCst), 2);
    assert_eq!(code_harness.calls.load(Ordering::SeqCst), 0);

    let unknown_profile_acl = profile.canonical_acl().replace(
        "agent_provider \"reference.echo\"",
        "agent_provider \"unknown.provider\"",
    );
    let unknown_profile =
        AgentProviderProfile::parse_acl(&unknown_profile_acl).expect("canonical unknown profile");
    assert_eq!(unknown_profile.kind(), "unknown.provider");
    let unknown_identity = AgentProviderRunIdentityV1::new(
        unknown_profile.digest().into(),
        unknown_profile.capability_digest().into(),
        format!("sha256:{}", "c".repeat(64)),
        "conversation-unknown".into(),
        "execution-unknown-attempt-1".into(),
    )
    .expect("unknown provider run identity");
    let unknown_binding = NodeAgentProviderRuntimeBindingV1 {
        execution_id: Uuid::now_v7(),
        provider_profile_acl: unknown_profile.canonical_acl().into(),
        provider_profile_digest: unknown_profile.digest().into(),
        provider_run_identity: unknown_identity.clone(),
        ..binding
    };
    let unknown_command = AgentProviderCommandV1::Start {
        request: AgentProviderRunStartV1::new(
            "execution-unknown:start".into(),
            unknown_identity,
            "This provider is not admitted.".into(),
        )
        .expect("unknown provider start command"),
    };
    let unknown_envelope = claim_command(
        node_id,
        unknown_binding.execution_id,
        3,
        unknown_binding.runtime_generation,
        NodeCommandPayload::AgentProviderCommand {
            binding: Box::new(unknown_binding),
            command: Box::new(unknown_command),
        },
    );
    let unknown = executor
        .execute(unknown_envelope)
        .await
        .expect("reject unknown provider deterministically");
    let NodeCommandOutcome::Rejected { failure } = unknown.outcome else {
        panic!("unknown provider must fail closed");
    };
    assert_eq!(failure.code, "invalid_agent_provider_harness_binding");
    assert!(!failure.retryable);
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 2);
    assert_eq!(provider_harness.calls.load(Ordering::SeqCst), 2);
    assert_eq!(code_harness.calls.load(Ordering::SeqCst), 0);
}
