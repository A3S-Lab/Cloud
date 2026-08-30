use super::types::{
    DispatchInput, DispatchOutput, ObserveInput, ObserveOutput, PreparedAgentExecution,
};
use super::*;
use crate::modules::agents::domain::{
    AcceptAgentProviderEventBatchWrite, AgentApprovalCheckpointStatus, AgentCodeRunBinding,
    AgentConversation, AgentConversationCreated, AgentEventContent, AgentExecution,
    AgentExecutionCancellationRequested, AgentExecutionCheckpointObjectError,
    AgentExecutionCheckpointObjectReference, AgentExecutionCheckpointObjectWrite,
    AgentExecutionEventDraft, AgentExecutionEventKind, AgentExecutionStarted,
    AgentProviderProfileBinding, AgentReleaseBinding, BindAgentCodeRunWrite,
    CreateAgentConversationWrite, IAgentExecutionCheckpointObjectStore, IAgentRepository,
    RecoverAgentCodeRunWrite, RequestAgentExecutionCancellationWrite, StartAgentExecutionWrite,
};
use crate::modules::agents::infrastructure::InMemoryAgentRepository;
use crate::modules::agents::BuiltInAgentExecutionProviderRegistry;
use crate::modules::fleet::domain::entities::EnrollmentToken;
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeEnrollmentDraft,
};
use crate::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName,
};
use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AgentConversationId, AgentExecutionId, AssetId, AssetReleaseId,
    BuildRunId, DeploymentId, EnrollmentTokenId, EnvironmentId, IdempotencyRequest, NodeId,
    OperationId, OrganizationId, PrincipalId, ProjectId, Sha256Digest, WorkloadId,
    WorkloadReplicaId, WorkloadRevisionId,
};
use crate::modules::workloads::infrastructure::InMemoryWorkloadRepository;
use a3s_cloud_contracts::{
    AgentProtocolRunIdentityV1, AgentProviderCapabilityV1, AgentProviderCommandReceiptV1,
    AgentProviderCommandV1, AgentProviderEventPageV1, AgentProviderEventRecordV1,
    AgentProviderProfile, AgentProviderRunStateV1, AgentProviderSemanticEventV1,
    AgentProviderToolPayloadIdentityV1, DomainEventEnvelope, HarnessAgentReleaseBindingV1,
    HarnessInvocationProfileV1, HarnessProviderBindingV1, HarnessToolBindingV1,
    HarnessWorkspaceBindingV1, NodeAgentProviderEventBatchV1, NodeCommandAck, NodeCommandEnvelope,
    NodeCommandLeaseRequest, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
    NodeHeartbeat, NodeObservationBatch, RuntimeObservationReport,
    REFERENCE_ECHO_AGENT_PROVIDER_KIND, REFERENCE_ECHO_AGENT_PROVIDER_PROTOCOL_V1,
};
use a3s_runtime::contract::{
    IsolationLevel, NetworkMode, ResourceControl, RuntimeCapabilities, RuntimeEvidence,
    RuntimeFeature, RuntimeHealthObservation, RuntimeHealthState, RuntimeObservation,
    RuntimeServiceEndpoint, RuntimeUnitClass, RuntimeUnitState,
};
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

mod approval_tests;
mod checkpoint_tests;
mod recovery_tests;

#[derive(Default)]
struct TestCheckpointObjects {
    bodies: RwLock<BTreeMap<String, Vec<u8>>>,
}

#[async_trait::async_trait]
impl IAgentExecutionCheckpointObjectStore for TestCheckpointObjects {
    async fn put(
        &self,
        reference: &AgentExecutionCheckpointObjectReference,
        body: Vec<u8>,
    ) -> Result<AgentExecutionCheckpointObjectWrite, AgentExecutionCheckpointObjectError> {
        let mut bodies = self.bodies.write().await;
        match bodies.get(&reference.object_ref) {
            Some(existing) if existing == &body => {
                Ok(AgentExecutionCheckpointObjectWrite { replayed: true })
            }
            Some(_) => Err(AgentExecutionCheckpointObjectError::Conflict(
                reference.object_ref.clone(),
            )),
            None => {
                bodies.insert(reference.object_ref.clone(), body);
                Ok(AgentExecutionCheckpointObjectWrite { replayed: false })
            }
        }
    }

    async fn get(
        &self,
        reference: &AgentExecutionCheckpointObjectReference,
    ) -> Result<Vec<u8>, AgentExecutionCheckpointObjectError> {
        self.bodies
            .read()
            .await
            .get(&reference.object_ref)
            .cloned()
            .ok_or(AgentExecutionCheckpointObjectError::NotFound)
    }
}

fn checkpoint_objects() -> Arc<dyn IAgentExecutionCheckpointObjectStore> {
    Arc::new(TestCheckpointObjects::default())
}

#[test]
fn agent_flow_uses_the_provider_contract_without_owning_a_run_lifecycle() {
    let source = [
        include_str!("approval.rs"),
        include_str!("binding.rs"),
        include_str!("recovery.rs"),
        include_str!("runtime.rs"),
    ]
    .join("\n");
    assert!(source.contains("NodeCommandPayload::AgentProviderCommand"));
    assert!(source.contains("NodeCommandPayload::CodeAgentCommand"));
    assert!(source.contains("AgentProviderCommandV1"));
    assert!(!source.contains("AgentProtocolCommandV1::Start"));
    assert!(!source.contains("AgentProtocolCommandV1::Cancel"));
    assert!(!source.contains("AgentProtocolCommandV1::Recover"));
    assert!(source.contains("a3s-code-cancel-v1"));
    assert!(source.contains("a3s-code-recover-v1"));
    assert!(source.contains("list_active_runtime_targets"));
    for forbidden in [
        "AgentSession",
        "AgentProtocolHost",
        "InMemoryRunStore",
        "spawn_run",
        "spawn_recovery",
        "cancel_run",
        "RunStore",
        "CreateAgentWorkloadDeployment",
        "CreateWorkloadDeployment",
        "create_deployment(",
        "RuntimeClient::apply",
    ] {
        assert!(
            !source.contains(forbidden),
            "Cloud Agent Flow must not own Code lifecycle primitive {forbidden}"
        );
    }
}

#[test]
fn agent_flow_configuration_is_bounded() {
    assert!(
        AgentExecutionFlowConfig::new(AgentExecutionFlowConfigOptions {
            heartbeat_timeout_ms: 1_000,
            command_ttl_ms: 10_000,
            observation_poll_ms: 1_000,
            convergence_timeout_ms: 60_000,
        })
        .is_ok()
    );
    assert!(
        AgentExecutionFlowConfig::new(AgentExecutionFlowConfigOptions {
            heartbeat_timeout_ms: 1_000,
            command_ttl_ms: 10_000,
            observation_poll_ms: 60_001,
            convergence_timeout_ms: 60_000,
        })
        .is_err()
    );
}

#[tokio::test]
async fn reference_provider_dispatch_preserves_the_common_profile_and_protocol() {
    let now = canonical_timestamp(Utc::now() - Duration::seconds(5));
    let organization_id = OrganizationId::new();
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id) =
        enroll_command_node(nodes.as_ref(), organization_id, now).await;
    let profile = AgentProviderProfile::parse_acl(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/a1.3/reference-echo-provider-profile.acl"
    )))
    .expect("reference provider profile");
    let provider =
        AgentProviderProfileBinding::from_profile(&profile).expect("reference provider binding");
    let agents = Arc::new(InMemoryAgentRepository::new());
    let (execution, _) = prepare_bound_execution_with_provider(
        agents.as_ref(),
        organization_id,
        node_id,
        now,
        provider,
    )
    .await;
    let runtime = AgentExecutionFlowRuntime::new(
        AgentExecutionFlowRuntimeDependencies {
            agents,
            checkpoint_objects: checkpoint_objects(),
            providers: Arc::new(
                BuiltInAgentExecutionProviderRegistry::new().expect("provider registry"),
            ),
            workload_targets: Arc::new(InMemoryWorkloadRepository::new()),
            node_control: nodes.clone(),
        },
        AgentExecutionFlowConfig::new(AgentExecutionFlowConfigOptions {
            heartbeat_timeout_ms: 60_000,
            command_ttl_ms: 60_000,
            observation_poll_ms: 1,
            convergence_timeout_ms: 60_000,
        })
        .expect("Agent Flow configuration"),
    );
    let mut legacy_binding =
        serde_json::to_value(execution.code.as_ref().expect("provider Runtime binding"))
            .expect("legacy binding JSON");
    legacy_binding
        .as_object_mut()
        .expect("provider binding object")
        .remove("invocation_profile");
    let mut legacy_execution = execution.clone();
    legacy_execution.code =
        Some(serde_json::from_value(legacy_binding).expect("legacy profile-less provider binding"));
    legacy_execution
        .validate()
        .expect("legacy provider execution remains readable");
    let legacy_error = super::runtime::start_command(&runtime, &legacy_execution)
        .await
        .expect_err("legacy profile-less provider start must fail closed");
    assert!(legacy_error
        .to_string()
        .contains("no immutable Harness invocation profile"));
    let prepared = PreparedAgentExecution {
        organization_id,
        execution_id: execution.id,
        binding: execution.code.clone().expect("provider Runtime binding"),
        runtime_started_at_ms: None,
    };
    let output = super::runtime::dispatch(
        &runtime,
        &execution.operation_id.to_string(),
        DispatchInput {
            prepared: Box::new(prepared),
        },
    )
    .await
    .expect("dispatch reference provider");
    assert!(matches!(output, DispatchOutput::Ready { .. }));

    let envelope = lease_code_command(nodes.as_ref(), node_id, agent_instance_id, 0).await;
    let NodeCommandPayload::AgentProviderCommand { binding, command } = envelope.payload else {
        panic!("reference provider must use the common provider command path");
    };
    let bound_profile = binding.profile().expect("bound provider profile");
    assert_eq!(bound_profile, profile);
    assert_eq!(bound_profile.kind(), REFERENCE_ECHO_AGENT_PROVIDER_KIND);
    assert_eq!(
        bound_profile.native_protocol(),
        REFERENCE_ECHO_AGENT_PROVIDER_PROTOCOL_V1
    );
    let AgentProviderCommandV1::Start { request } = command.as_ref() else {
        panic!("reference provider must receive a start command");
    };
    let invocation = request
        .invocation_profile
        .as_ref()
        .expect("new provider run must carry its immutable invocation profile");
    let invocation_digest = invocation
        .digest()
        .expect("Harness invocation profile digest");
    assert_eq!(
        request.identity.invocation_profile_digest.as_deref(),
        Some(invocation_digest.as_str())
    );
    assert_eq!(invocation.provider.profile_digest, profile.digest());
    assert!(binding.code_binding().is_err());
}

#[tokio::test]
async fn provider_process_restart_recovers_before_cancelling_the_new_run() {
    let now = canonical_timestamp(Utc::now() - Duration::seconds(5));
    let organization_id = OrganizationId::new();
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id) =
        enroll_command_node(nodes.as_ref(), organization_id, now).await;
    let agents = Arc::new(InMemoryAgentRepository::new());
    let (execution, binding) =
        prepare_bound_execution(agents.as_ref(), organization_id, node_id, now).await;
    let flow_runtime = AgentExecutionFlowRuntime::new(
        AgentExecutionFlowRuntimeDependencies {
            agents: agents.clone(),
            checkpoint_objects: checkpoint_objects(),
            providers: Arc::new(
                BuiltInAgentExecutionProviderRegistry::new().expect("provider registry"),
            ),
            workload_targets: Arc::new(InMemoryWorkloadRepository::new()),
            node_control: nodes.clone(),
        },
        AgentExecutionFlowConfig::new(AgentExecutionFlowConfigOptions {
            heartbeat_timeout_ms: 60_000,
            command_ttl_ms: 60_000,
            observation_poll_ms: 1,
            convergence_timeout_ms: 60_000,
        })
        .expect("Agent Flow configuration"),
    );
    let observed_at = canonical_timestamp(Utc::now());
    let restarted_at_ms = u64::try_from(observed_at.timestamp_millis())
        .expect("Runtime process timestamp")
        .saturating_sub(1);
    let prepared = PreparedAgentExecution {
        organization_id,
        execution_id: execution.id,
        binding: binding.clone(),
        runtime_started_at_ms: Some(restarted_at_ms.saturating_sub(1_000)),
    };
    let dispatched = match super::runtime::dispatch(
        &flow_runtime,
        &execution.operation_id.to_string(),
        DispatchInput {
            prepared: Box::new(prepared),
        },
    )
    .await
    .expect("dispatch Code start")
    {
        DispatchOutput::Ready { dispatched } => *dispatched,
        DispatchOutput::Terminal { .. } => panic!("active execution must dispatch"),
    };
    let start = lease_and_ack_code_command(
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        0,
        AgentCommandKind::Start,
    )
    .await;
    assert_eq!(dispatched.command_id.as_uuid(), start.command_id);

    let current = agents
        .find_execution(organization_id, execution.id)
        .await
        .expect("find execution before cancellation")
        .expect("execution before cancellation");
    let expected_version = current.aggregate_version;
    let mut cancelling = current.clone();
    let cancellation_at = observed_at.max(current.updated_at);
    cancelling
        .request_cancellation(cancellation_at)
        .expect("request cancellation");
    agents
        .request_cancellation(RequestAgentExecutionCancellationWrite {
            event: AgentExecutionCancellationRequested::envelope(&cancelling, Uuid::now_v7())
                .expect("cancellation event"),
            execution: cancelling,
            expected_version,
            idempotency: idempotency("agent-flow-cancellation", "cancel", b"cancel"),
        })
        .await
        .expect("persist cancellation");
    record_running_observation(
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        &binding,
        restarted_at_ms,
        observed_at,
    )
    .await;

    let recovery_dispatched = match super::runtime::observe(
        &flow_runtime,
        &execution.operation_id.to_string(),
        ObserveInput {
            dispatched: Box::new(dispatched.clone()),
        },
    )
    .await
    .expect("observe restarted provider")
    {
        ObserveOutput::Pending {
            dispatched: Some(dispatched),
            ..
        } => *dispatched,
        _ => panic!("provider restart must persist a recovery dispatch"),
    };
    assert_eq!(
        recovery_dispatched.recovery_checkpoint_run_id.as_deref(),
        Some(binding.identity().run_id.as_str())
    );
    assert_eq!(
        recovery_dispatched.prepared.runtime_started_at_ms,
        Some(restarted_at_ms)
    );
    let recovered_run_id =
        AgentCodeRunBinding::recovery_run_id(execution.id, &binding.identity().run_id);
    assert_eq!(
        recovery_dispatched.prepared.binding.identity().run_id,
        recovered_run_id
    );
    assert_ne!(recovery_dispatched.command_id, dispatched.command_id);

    let recover = lease_and_ack_code_command(
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        start.sequence,
        AgentCommandKind::Recover,
    )
    .await;
    assert_eq!(recovery_dispatched.command_id.as_uuid(), recover.command_id);
    let NodeCommandPayload::AgentProviderCommand {
        binding: recovered_node_binding,
        command: recovered_command,
    } = &recover.payload
    else {
        panic!("recovery must use the Agent provider command path");
    };
    assert_eq!(
        recovered_node_binding.provider_run_identity.run_id,
        recovered_run_id
    );
    let AgentProviderCommandV1::Recover { request } = recovered_command.as_ref() else {
        panic!("recovery command expected");
    };
    assert_eq!(request.checkpoint_run_id, binding.identity().run_id);

    match super::runtime::observe(
        &flow_runtime,
        &execution.operation_id.to_string(),
        ObserveInput {
            dispatched: Box::new(recovery_dispatched.clone()),
        },
    )
    .await
    .expect("observe acknowledged recovery while cancelling")
    {
        ObserveOutput::Pending {
            dispatched: None, ..
        } => {}
        _ => panic!("cancellation must continue on the recovered run"),
    }
    let cancel =
        lease_code_command(nodes.as_ref(), node_id, agent_instance_id, recover.sequence).await;
    assert_ne!(cancel.command_id, start.command_id);
    assert_ne!(cancel.command_id, recover.command_id);
    let NodeCommandPayload::AgentProviderCommand {
        binding: cancelled_binding,
        command: cancel_command,
    } = &cancel.payload
    else {
        panic!("cancellation must use the Agent provider command path");
    };
    assert_eq!(
        cancelled_binding.provider_run_identity.run_id,
        recovered_run_id
    );
    let AgentProviderCommandV1::Cancel { request } = cancel_command.as_ref() else {
        panic!("cancel command expected");
    };
    assert_eq!(request.identity.run_id, recovered_run_id);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentCommandKind {
    Start,
    Recover,
    Resume,
}

async fn prepare_bound_execution(
    agents: &InMemoryAgentRepository,
    organization_id: OrganizationId,
    node_id: NodeId,
    requested_at: chrono::DateTime<Utc>,
) -> (AgentExecution, AgentCodeRunBinding) {
    prepare_bound_execution_with_provider(
        agents,
        organization_id,
        node_id,
        requested_at,
        AgentProviderProfileBinding::native_code().expect("native Code provider"),
    )
    .await
}

async fn prepare_bound_execution_with_provider(
    agents: &InMemoryAgentRepository,
    organization_id: OrganizationId,
    node_id: NodeId,
    requested_at: chrono::DateTime<Utc>,
    provider: AgentProviderProfileBinding,
) -> (AgentExecution, AgentCodeRunBinding) {
    prepare_bound_execution_with_provider_and_tools(
        agents,
        organization_id,
        node_id,
        requested_at,
        provider,
        Vec::new(),
    )
    .await
}

async fn prepare_bound_execution_with_provider_and_tools(
    agents: &InMemoryAgentRepository,
    organization_id: OrganizationId,
    node_id: NodeId,
    requested_at: chrono::DateTime<Utc>,
    provider: AgentProviderProfileBinding,
    tools: Vec<HarnessToolBindingV1>,
) -> (AgentExecution, AgentCodeRunBinding) {
    let conversation = AgentConversation::create(
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        AgentConversationId::new(),
        requested_at,
    )
    .expect("Agent conversation");
    agents
        .create_conversation(CreateAgentConversationWrite {
            event: AgentConversationCreated::envelope(&conversation, Uuid::now_v7())
                .expect("conversation event"),
            conversation: conversation.clone(),
            idempotency: idempotency("agent-flow-conversations", "create", b"create"),
        })
        .await
        .expect("create conversation");
    let release_digest =
        Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("release digest");
    let release = AgentReleaseBinding::new(
        organization_id,
        AssetId::new(),
        AssetReleaseId::new(),
        BuildRunId::new(),
        format!("oci://registry.example/agents/demo@{release_digest}"),
        release_digest,
        "application/vnd.oci.image.manifest.v1+json",
        1,
    )
    .expect("Agent release binding");
    let execution = AgentExecution::create_with_provider(
        organization_id,
        conversation.id,
        AgentExecutionId::new(),
        OperationId::new(),
        release,
        provider.clone(),
        requested_at,
    )
    .expect("Agent execution");
    agents
        .start_execution(StartAgentExecutionWrite {
            initial_event: AgentExecutionEventDraft::new(
                AgentExecutionEventKind::ExecutionRequested,
                AgentEventContent::inline_json(serde_json::json!({"prompt": "hello"}))
                    .expect("event content"),
                requested_at,
            )
            .expect("initial event"),
            event: AgentExecutionStarted::envelope(&execution, Uuid::now_v7())
                .expect("execution event"),
            execution: execution.clone(),
            idempotency: idempotency("agent-flow-executions", "start", b"start"),
        })
        .await
        .expect("start execution");
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let runtime_spec_digest =
        Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("Runtime digest");
    let mut required_capabilities = vec![
        AgentProviderCapabilityV1::Cancellation,
        AgentProviderCapabilityV1::Cleanup,
        AgentProviderCapabilityV1::EventPages,
    ];
    if !tools.is_empty() {
        required_capabilities.push(AgentProviderCapabilityV1::ToolCalls);
    }
    if tools.iter().any(|tool| tool.approval_required) {
        required_capabilities.push(AgentProviderCapabilityV1::PauseResume);
    }
    required_capabilities.sort_by_key(|capability| capability.as_str());
    let invocation = HarnessInvocationProfileV1 {
        schema: HarnessInvocationProfileV1::SCHEMA.into(),
        agent: HarnessAgentReleaseBindingV1 {
            organization_id: execution.organization_id.as_uuid(),
            asset_id: execution.agent.asset_id().as_uuid(),
            asset_release_id: execution.agent.asset_release_id().as_uuid(),
            build_run_id: execution.agent.build_run_id().as_uuid(),
            artifact_digest: execution.agent.artifact_digest().as_str().into(),
        },
        provider: HarnessProviderBindingV1 {
            kind: provider.kind().into(),
            revision: provider.revision().into(),
            profile_digest: provider.profile_digest().into(),
            capability_digest: provider.capability_digest().into(),
        },
        instructions_digest: execution.agent.artifact_digest().as_str().into(),
        environment_policy_digest: format!("sha256:{}", "c".repeat(64)),
        security_policy_digest: format!("sha256:{}", "d".repeat(64)),
        workspace: HarnessWorkspaceBindingV1 {
            workload_id: workload_id.as_uuid(),
            workload_revision_id: workload_revision_id.as_uuid(),
            runtime_unit_id: "agent-runtime:revision:1".into(),
            runtime_generation: 1,
            runtime_spec_digest: runtime_spec_digest.as_str().into(),
            working_directory: Some("/workspace".into()),
        },
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        models: Vec::new(),
        secrets: Vec::new(),
        tools,
        required_capabilities,
    };
    let binding = AgentCodeRunBinding::new_with_provider(
        provider.clone(),
        node_id,
        workload_id,
        workload_revision_id,
        DeploymentId::new(),
        WorkloadReplicaId::new(),
        "agent-runtime:revision:1",
        1,
        runtime_spec_digest,
        "agent",
        AgentProtocolRunIdentityV1 {
            schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
            protocol: provider.native_protocol().into(),
            agent_release_identity: execution.agent.artifact_digest().as_str().into(),
            session_id: format!("agent-conversation-{}", conversation.id),
            run_id: format!("agent-execution-{}", execution.id),
        },
        requested_at + Duration::seconds(1),
    )
    .expect("Code run binding")
    .with_invocation_profile(invocation)
    .expect("Harness invocation profile");
    let write = agents
        .bind_code_run(BindAgentCodeRunWrite {
            organization_id,
            execution_id: execution.id,
            binding: binding.clone(),
        })
        .await
        .expect("bind Code run");
    (write.execution, binding)
}

async fn enroll_command_node(
    nodes: &InMemoryNodeRepository,
    organization_id: OrganizationId,
    requested_at: chrono::DateTime<Utc>,
) -> (NodeId, Uuid) {
    let credential = EnrollmentTokenCredential::from_secret(&format!("a3sn_{}", "9".repeat(64)))
        .expect("enrollment credential");
    let token = EnrollmentToken::new(
        EnrollmentTokenId::new(),
        organization_id,
        "Agent Flow worker",
        credential.clone(),
        requested_at,
        requested_at + Duration::minutes(10),
    )
    .expect("enrollment token");
    nodes
        .issue_enrollment_token(
            token.clone(),
            domain_event(
                organization_id,
                token.id.as_uuid(),
                token.aggregate_version,
                "fleet.enrollment-token.issued",
            ),
            idempotency("agent-flow-node-tokens", "issue", b"issue"),
        )
        .await
        .expect("issue enrollment token");
    let agent_instance_id = Uuid::now_v7();
    let reservation = nodes
        .reserve_enrollment(
            &credential,
            NodeEnrollmentDraft {
                proposed_node_id: NodeId::new(),
                name: NodeName::new("agent-flow-worker").expect("node name"),
                agent_instance_id,
                agent_version: "0.1.0".into(),
                capabilities: NodeCapabilities::new(
                    "a3s-box",
                    "agent-flow-test",
                    serde_json::json!({
                        "schema": a3s_runtime::contract::RuntimeCapabilities::SCHEMA,
                        "provider_id": "a3s-box",
                        "provider_build": "agent-flow-test"
                    }),
                )
                .expect("node capabilities"),
                request_digest: format!("sha256:{}", "8".repeat(64)),
                requested_at,
            },
        )
        .await
        .expect("reserve node enrollment");
    (reservation.node.id, agent_instance_id)
}

async fn record_running_observation(
    nodes: &InMemoryNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    binding: &AgentCodeRunBinding,
    started_at_ms: u64,
    observed_at: chrono::DateTime<Utc>,
) {
    let endpoint = RuntimeServiceEndpoint::node_local_tcp(binding.service_port_name(), 49_152)
        .expect("node-local Agent endpoint");
    let mut claims = BTreeMap::new();
    endpoint
        .insert_claim(&mut claims)
        .expect("Runtime endpoint claim");
    let observed_at_ms =
        u64::try_from(observed_at.timestamp_millis()).expect("observation timestamp");
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: binding.runtime_unit_id().into(),
        generation: binding.runtime_generation(),
        spec_digest: binding.runtime_spec_digest().as_str().into(),
        class: RuntimeUnitClass::Service,
        state: RuntimeUnitState::Running,
        provider_resource_id: Some("agent-flow-provider".into()),
        provider_build: Some("agent-flow-test".into()),
        observed_at_ms,
        started_at_ms: Some(started_at_ms),
        finished_at_ms: None,
        health: Some(RuntimeHealthObservation {
            state: RuntimeHealthState::Healthy,
            checked_at_ms: observed_at_ms,
            message: None,
        }),
        outputs: Vec::new(),
        usage: None,
        evidence: Some(RuntimeEvidence {
            provider_build: "agent-flow-test".into(),
            spec_digest: binding.runtime_spec_digest().as_str().into(),
            semantics_profile_digest: None,
            identity_attachment_digest: None,
            claims,
        }),
        provider_attestation: None,
        failure: None,
    };
    observation.validate().expect("Runtime observation");
    nodes
        .record_observations(
            NodeObservationBatch {
                schema: NodeObservationBatch::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                sent_at: observed_at,
                heartbeat: NodeHeartbeat {
                    schema: NodeHeartbeat::SCHEMA.into(),
                    node_id: node_id.as_uuid(),
                    agent_instance_id,
                    observed_at,
                    agent_version: "0.1.0".into(),
                    runtime_capabilities: runtime_capabilities(),
                },
                observations: vec![RuntimeObservationReport {
                    report_id: Uuid::now_v7(),
                    command_id: None,
                    observed_at,
                    observation,
                }],
            }
            .into(),
            observed_at,
        )
        .await
        .expect("record restarted Runtime observation");
}

fn runtime_capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: a3s_runtime::ProviderId::parse("a3s-box").expect("Runtime provider ID"),
        provider_build: "agent-flow-test".into(),
        unit_classes: vec![RuntimeUnitClass::Task, RuntimeUnitClass::Service],
        artifact_media_types: vec!["application/vnd.oci.image.manifest.v1+json".into()],
        isolation_levels: vec![IsolationLevel::Sandbox],
        network_modes: vec![NetworkMode::None, NetworkMode::Service],
        mount_kinds: Vec::new(),
        health_check_kinds: Vec::new(),
        resource_controls: vec![ResourceControl::Cpu, ResourceControl::Memory],
        features: vec![
            RuntimeFeature::DurableIdentity,
            RuntimeFeature::Stop,
            RuntimeFeature::Remove,
            RuntimeFeature::ServiceTcp,
        ],
    }
}

async fn lease_and_ack_code_command(
    nodes: &InMemoryNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    after_sequence: u64,
    expected_kind: AgentCommandKind,
) -> NodeCommandEnvelope {
    let envelope = lease_code_command(nodes, node_id, agent_instance_id, after_sequence).await;
    let NodeCommandPayload::AgentProviderCommand {
        binding, command, ..
    } = &envelope.payload
    else {
        panic!("leased command must be an Agent provider command");
    };
    assert!(matches!(
        (expected_kind, command.as_ref()),
        (
            AgentCommandKind::Start,
            AgentProviderCommandV1::Start { .. }
        ) | (
            AgentCommandKind::Recover,
            AgentProviderCommandV1::Recover { .. }
        ) | (
            AgentCommandKind::Resume,
            AgentProviderCommandV1::Resume { .. }
        )
    ));
    let earliest_evidence_at = envelope
        .issued_at
        .checked_add_signed(Duration::milliseconds(1))
        .expect("command evidence timestamp");
    let completed_at = canonical_timestamp(Utc::now().max(earliest_evidence_at));
    let receipt_state = match expected_kind {
        AgentCommandKind::Start | AgentCommandKind::Recover => AgentProviderRunStateV1::Created,
        AgentCommandKind::Resume => AgentProviderRunStateV1::Executing,
    };
    let receipt = AgentProviderCommandReceiptV1::accepted(
        &binding.profile().expect("provider profile"),
        command,
        receipt_state,
        u64::try_from(completed_at.timestamp_millis()).expect("command completion timestamp"),
        false,
    )
    .expect("provider command receipt");
    nodes
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: envelope.command_id,
                lease_id: envelope.lease_id,
                node_id: envelope.node_id,
                sequence: envelope.sequence,
                payload_digest: envelope.payload_digest.clone(),
                completed_at,
                outcome: NodeCommandOutcome::Succeeded {
                    result: Box::new(NodeCommandResult::AgentProviderCommandAccepted {
                        receipt: Box::new(receipt),
                    }),
                },
            },
            completed_at,
        )
        .await
        .expect("acknowledge Code command");
    envelope
}

async fn lease_code_command(
    nodes: &InMemoryNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    after_sequence: u64,
) -> NodeCommandEnvelope {
    let now = canonical_timestamp(Utc::now());
    let lease = nodes
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence,
                max_commands: 1,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            now,
            now + Duration::seconds(10),
        )
        .await
        .expect("lease Code command");
    assert_eq!(lease.commands.len(), 1);
    lease.commands.into_iter().next().expect("Code command")
}

fn domain_event(
    organization_id: OrganizationId,
    aggregate_id: Uuid,
    aggregate_version: u64,
    event_key: &str,
) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: event_key.into(),
        schema_version: 1,
        scope: a3s_cloud_contracts::CloudScopeRef::Organization {
            organization_id: organization_id.as_uuid(),
        },
        aggregate_id,
        aggregate_version,
        occurred_at: canonical_timestamp(Utc::now()),
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        payload: serde_json::json!({}),
    }
}

fn idempotency(scope: &str, key: &str, body: &[u8]) -> IdempotencyRequest {
    IdempotencyRequest::new(scope, key, body).expect("idempotency")
}
