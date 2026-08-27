use super::*;
use crate::modules::agents::application::{
    DecideAgentApprovalCheckpoint, DecideAgentApprovalCheckpointHandler,
};
use crate::modules::agents::domain::IAgentApprovalCheckpointRepository;
use crate::modules::identity::domain::repositories::IResourceAuthorizationDecisionRepository;
use crate::modules::identity::domain::services::{
    ResourceAccessEvaluator, ResourceAuthorizationDecisionRequest,
};
use crate::modules::shared_kernel::domain::{
    ApiTokenId, AuthorizationDecisionRef, RepositoryError,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct RecordingApprovalAuthorizer {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl IResourceAuthorizationDecisionRepository for RecordingApprovalAuthorizer {
    async fn authorize_resource(
        &self,
        request: ResourceAuthorizationDecisionRequest,
    ) -> Result<AuthorizationDecisionRef, RepositoryError> {
        assert_eq!(request.action, "agent.execution.approval.decide");
        self.calls.fetch_add(1, Ordering::SeqCst);
        AuthorizationDecisionRef::new(
            format!("agent-flow-approval-authorization:{}", request.request_id),
            Sha256Digest::parse(format!("sha256:{}", "9".repeat(64)))
                .map_err(RepositoryError::Storage)?,
        )
        .map_err(RepositoryError::Storage)
    }
}

#[tokio::test]
async fn approval_checkpoint_resumes_only_after_one_exact_durable_decision() {
    let requested_at = canonical_timestamp(Utc::now() - Duration::seconds(5));
    let organization_id = OrganizationId::new();
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id) =
        enroll_command_node(nodes.as_ref(), organization_id, requested_at).await;
    let profile = AgentProviderProfile::parse_acl(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/a1.3/reference-echo-provider-profile.acl"
    )))
    .expect("reference provider profile");
    let provider =
        AgentProviderProfileBinding::from_profile(&profile).expect("reference provider binding");
    let tool = HarnessToolBindingV1 {
        name: "workspace.publish".into(),
        revision: "1.0.0".into(),
        contract_digest: format!("sha256:{}", "e".repeat(64)),
        approval_required: true,
    };
    let agents = Arc::new(InMemoryAgentRepository::new());
    let (execution, binding) = prepare_bound_execution_with_provider_and_tools(
        agents.as_ref(),
        organization_id,
        node_id,
        requested_at,
        provider,
        vec![tool.clone()],
    )
    .await;
    let flow_runtime = AgentExecutionFlowRuntime::new(
        AgentExecutionFlowRuntimeDependencies {
            agents: agents.clone(),
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
    let prepared = PreparedAgentExecution {
        organization_id,
        execution_id: execution.id,
        binding: binding.clone(),
        runtime_started_at_ms: None,
    };
    let dispatched = match super::super::runtime::dispatch(
        &flow_runtime,
        &execution.operation_id.to_string(),
        DispatchInput {
            prepared: Box::new(prepared),
        },
    )
    .await
    .expect("dispatch provider start")
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
    let runtime_observed_at = canonical_timestamp(Utc::now());
    record_running_observation(
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        &binding,
        u64::try_from(binding.bound_at().timestamp_millis()).expect("Runtime start time"),
        runtime_observed_at,
    )
    .await;

    let identity = binding.provider_identity().expect("provider identity");
    let event_at_ms =
        u64::try_from(binding.bound_at().timestamp_millis()).expect("provider event time") + 1;
    let approval_page = AgentProviderEventPageV1 {
        schema: AgentProviderEventPageV1::SCHEMA.into(),
        identity: identity.clone(),
        after_event_sequence: None,
        first_available_sequence: Some(0),
        source_first_sequence: Some(0),
        source_last_sequence: Some(0),
        source_event_count: 1,
        latest_sequence_exclusive: 1,
        next_after_event_sequence: Some(0),
        state: AgentProviderRunStateV1::AwaitingApproval,
        observed_at_ms: event_at_ms,
        retention_gap: false,
        has_more: false,
        terminal_failure: None,
        events: vec![AgentProviderEventRecordV1 {
            sequence: 0,
            occurred_at_ms: event_at_ms,
            event: AgentProviderSemanticEventV1::ToolRequest {
                call_id: "publish-1".into(),
                tool: tool.clone(),
                request: AgentProviderToolPayloadIdentityV1 {
                    digest: format!("sha256:{}", "f".repeat(64)),
                    size_bytes: 128,
                    media_type: "application/json".into(),
                },
            },
        }],
    };
    let approval_batch = NodeAgentProviderEventBatchV1 {
        schema: NodeAgentProviderEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: node_id.as_uuid(),
        binding: binding
            .node_provider_runtime_binding(execution.id.as_uuid())
            .expect("provider Runtime binding"),
        page: approval_page,
        sent_at_ms: event_at_ms + 1,
    };
    approval_batch.validate().expect("approval event batch");
    let approval_accepted_at = DateTime::<Utc>::from_timestamp_millis(
        i64::try_from(approval_batch.sent_at_ms + 1).expect("approval acceptance time"),
    )
    .expect("approval acceptance timestamp");
    agents
        .accept_provider_event_batch(
            AcceptAgentProviderEventBatchWrite::new(
                organization_id,
                node_id,
                approval_batch,
                approval_accepted_at,
            )
            .expect("approval event write"),
        )
        .await
        .expect("accept approval request");
    let checkpoint = agents
        .find_active_checkpoint(organization_id, execution.id)
        .await
        .expect("find active approval")
        .expect("active approval");
    assert_eq!(checkpoint.status, AgentApprovalCheckpointStatus::Pending);

    let retention_gap_at_ms = event_at_ms + 2;
    let retention_gap = NodeAgentProviderEventBatchV1 {
        schema: NodeAgentProviderEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: node_id.as_uuid(),
        binding: binding
            .node_provider_runtime_binding(execution.id.as_uuid())
            .expect("provider Runtime binding"),
        page: AgentProviderEventPageV1 {
            schema: AgentProviderEventPageV1::SCHEMA.into(),
            identity: binding.provider_identity().expect("provider identity"),
            after_event_sequence: Some(0),
            first_available_sequence: Some(2),
            source_first_sequence: None,
            source_last_sequence: None,
            source_event_count: 0,
            latest_sequence_exclusive: 2,
            next_after_event_sequence: Some(0),
            state: AgentProviderRunStateV1::AwaitingApproval,
            observed_at_ms: retention_gap_at_ms,
            retention_gap: true,
            has_more: false,
            terminal_failure: None,
            events: Vec::new(),
        },
        sent_at_ms: retention_gap_at_ms + 1,
    };
    retention_gap.validate().expect("retention-gap event batch");
    let rejection = agents
        .accept_provider_event_batch(
            AcceptAgentProviderEventBatchWrite::new(
                organization_id,
                node_id,
                retention_gap,
                DateTime::<Utc>::from_timestamp_millis(
                    i64::try_from(retention_gap_at_ms + 2).expect("retention-gap acceptance time"),
                )
                .expect("retention-gap acceptance timestamp"),
            )
            .expect("retention-gap event write"),
        )
        .await
        .expect_err("active approval must reject provider recovery");
    assert!(rejection
        .to_string()
        .contains("paused Agent provider advanced before exact resume"));
    assert!(agents
        .find_execution(organization_id, execution.id)
        .await
        .expect("find execution after rejected recovery")
        .expect("execution after rejected recovery")
        .code
        .as_ref()
        .is_some_and(|current| current.has_same_run_binding(&binding)));

    let mut dispatched = match super::super::runtime::observe(
        &flow_runtime,
        &execution.operation_id.to_string(),
        ObserveInput {
            dispatched: Box::new(dispatched),
        },
    )
    .await
    .expect("record provider process")
    {
        ObserveOutput::Pending {
            dispatched: Some(dispatched),
            ..
        } => *dispatched,
        _ => panic!("provider process identity must be persisted"),
    };
    match super::super::runtime::observe(
        &flow_runtime,
        &execution.operation_id.to_string(),
        ObserveInput {
            dispatched: Box::new(dispatched.clone()),
        },
    )
    .await
    .expect("observe pending approval")
    {
        ObserveOutput::Pending {
            dispatched: None, ..
        } => {}
        _ => panic!("pending approval must not dispatch a resume command"),
    }

    let decision_at = canonical_timestamp(Utc::now().max(checkpoint.updated_at));
    let requested_decision_at = decision_at + Duration::nanoseconds(1);
    let authorizer = Arc::new(RecordingApprovalAuthorizer::default());
    let decision_handler =
        DecideAgentApprovalCheckpointHandler::new(agents.clone(), authorizer.clone());
    let decision = DecideAgentApprovalCheckpoint {
        organization_id,
        execution_id: execution.id,
        checkpoint_id: checkpoint.id,
        expected_version: checkpoint.aggregate_version,
        outcome: a3s_cloud_contracts::AgentProviderApprovalOutcomeV1::Approved,
        reason: Some("release approved".into()),
        resource_access: ResourceAccessEvaluator::organization_wide(),
        actor_principal_id: PrincipalId::new(),
        credential_id: ApiTokenId::new(),
        actor_is_platform_admin: false,
        idempotency_key: "approve-publish-1".into(),
        request_id: Uuid::now_v7(),
        requested_at: requested_decision_at,
    };
    let decided = decision_handler
        .execute(decision.clone(), CqrsContext::new(ModuleRef::new()))
        .await
        .expect("approval decision transport")
        .expect("approve Tool request");
    assert!(!decided.replayed);
    assert_eq!(
        decided.checkpoint.status,
        AgentApprovalCheckpointStatus::Approved
    );
    assert_eq!(decided.checkpoint.decided_at, Some(decision_at));
    let replayed = decision_handler
        .execute(decision, CqrsContext::new(ModuleRef::new()))
        .await
        .expect("approval replay transport")
        .expect("replay Tool approval");
    assert!(replayed.replayed);
    assert_eq!(replayed.checkpoint, decided.checkpoint);
    assert_eq!(authorizer.calls.load(Ordering::SeqCst), 1);

    dispatched = match super::super::runtime::observe(
        &flow_runtime,
        &execution.operation_id.to_string(),
        ObserveInput {
            dispatched: Box::new(dispatched),
        },
    )
    .await
    .expect("dispatch exact approval resume")
    {
        ObserveOutput::Pending {
            dispatched: Some(dispatched),
            ..
        } => *dispatched,
        _ => panic!("decided approval must persist a resume dispatch"),
    };
    let approval_dispatch = dispatched
        .approval
        .as_ref()
        .expect("approval resume dispatch")
        .clone();
    assert_eq!(approval_dispatch.checkpoint_id, checkpoint.id);
    let resume = lease_and_ack_code_command(
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        start.sequence,
        AgentCommandKind::Resume,
    )
    .await;
    assert_eq!(approval_dispatch.command_id.as_uuid(), resume.command_id);
    match &resume.payload {
        NodeCommandPayload::AgentProviderCommand { command, .. } => {
            let AgentProviderCommandV1::Resume { request } = command.as_ref() else {
                panic!("approval must dispatch a provider resume command");
            };
            assert_eq!(request.decision.checkpoint_id, checkpoint.id.to_string());
            assert_eq!(
                request.decision.outcome,
                a3s_cloud_contracts::AgentProviderApprovalOutcomeV1::Approved
            );
        }
        _ => panic!("approval must use the provider-neutral command path"),
    }

    let unsettled_dispatch = dispatched.clone();
    dispatched = match super::super::runtime::observe(
        &flow_runtime,
        &execution.operation_id.to_string(),
        ObserveInput {
            dispatched: Box::new(dispatched.clone()),
        },
    )
    .await
    .expect("settle exact approval resume")
    {
        ObserveOutput::Pending {
            dispatched: Some(dispatched),
            ..
        } => *dispatched,
        _ => panic!("settled approval must preserve provider observation state"),
    };
    assert!(dispatched.approval.is_none());
    let resumed = agents
        .find_checkpoint(organization_id, checkpoint.id)
        .await
        .expect("find resumed approval")
        .expect("resumed approval");
    assert_eq!(resumed.status, AgentApprovalCheckpointStatus::Resumed);
    assert_eq!(
        resumed.resume_command_id,
        Some(approval_dispatch.command_id)
    );
    let resumed_execution = agents
        .find_execution(organization_id, execution.id)
        .await
        .expect("find resumed execution")
        .expect("resumed execution");
    assert_eq!(
        resumed_execution.status,
        crate::modules::agents::domain::AgentExecutionStatus::Running
    );

    match super::super::runtime::observe(
        &flow_runtime,
        &execution.operation_id.to_string(),
        ObserveInput {
            dispatched: Box::new(unsettled_dispatch),
        },
    )
    .await
    .expect("replay exact approval settlement")
    {
        ObserveOutput::Pending {
            dispatched: Some(replayed),
            ..
        } => assert!(replayed.approval.is_none()),
        _ => panic!("approval settlement replay must preserve provider observation state"),
    }
    assert_eq!(
        agents
            .find_checkpoint(organization_id, checkpoint.id)
            .await
            .expect("find replayed approval")
            .expect("replayed approval"),
        resumed
    );
    assert_eq!(
        agents
            .find_execution(organization_id, execution.id)
            .await
            .expect("find replayed execution")
            .expect("replayed execution"),
        resumed_execution
    );

    let continued_at_ms = event_at_ms + 1;
    let continued_page = AgentProviderEventPageV1 {
        schema: AgentProviderEventPageV1::SCHEMA.into(),
        identity,
        after_event_sequence: Some(0),
        first_available_sequence: Some(0),
        source_first_sequence: None,
        source_last_sequence: None,
        source_event_count: 0,
        latest_sequence_exclusive: 1,
        next_after_event_sequence: Some(0),
        state: AgentProviderRunStateV1::Executing,
        observed_at_ms: continued_at_ms,
        retention_gap: false,
        has_more: false,
        terminal_failure: None,
        events: Vec::new(),
    };
    let continued_batch = NodeAgentProviderEventBatchV1 {
        schema: NodeAgentProviderEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: node_id.as_uuid(),
        binding: binding
            .node_provider_runtime_binding(execution.id.as_uuid())
            .expect("provider Runtime binding"),
        page: continued_page,
        sent_at_ms: continued_at_ms + 1,
    };
    continued_batch
        .validate()
        .expect("continued provider batch");
    let continued_accepted_at = DateTime::<Utc>::from_timestamp_millis(
        i64::try_from(continued_batch.sent_at_ms + 1).expect("continued acceptance time"),
    )
    .expect("continued acceptance timestamp");
    agents
        .accept_provider_event_batch(
            AcceptAgentProviderEventBatchWrite::new(
                organization_id,
                node_id,
                continued_batch,
                continued_accepted_at,
            )
            .expect("continued provider write"),
        )
        .await
        .expect("accept provider continuation");
    let current = agents
        .find_execution(organization_id, execution.id)
        .await
        .expect("find continued execution")
        .expect("continued execution");
    assert_eq!(
        current.status,
        crate::modules::agents::domain::AgentExecutionStatus::Running
    );
    match super::super::runtime::observe(
        &flow_runtime,
        &execution.operation_id.to_string(),
        ObserveInput {
            dispatched: Box::new(dispatched),
        },
    )
    .await
    .expect("observe provider continuation")
    {
        ObserveOutput::Pending {
            dispatched: None, ..
        } => {}
        _ => panic!("provider continuation must retain the settled dispatch state"),
    }
}

#[tokio::test]
async fn provider_restart_closes_pending_approval_without_dispatching_recovery() {
    let requested_at = canonical_timestamp(Utc::now() - Duration::seconds(5));
    let organization_id = OrganizationId::new();
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id) =
        enroll_command_node(nodes.as_ref(), organization_id, requested_at).await;
    let profile = AgentProviderProfile::parse_acl(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/a1.3/reference-echo-provider-profile.acl"
    )))
    .expect("reference provider profile");
    let provider =
        AgentProviderProfileBinding::from_profile(&profile).expect("reference provider binding");
    let tool = HarnessToolBindingV1 {
        name: "workspace.publish".into(),
        revision: "1.0.0".into(),
        contract_digest: format!("sha256:{}", "e".repeat(64)),
        approval_required: true,
    };
    let agents = Arc::new(InMemoryAgentRepository::new());
    let (execution, binding) = prepare_bound_execution_with_provider_and_tools(
        agents.as_ref(),
        organization_id,
        node_id,
        requested_at,
        provider,
        vec![tool.clone()],
    )
    .await;
    let flow_runtime = AgentExecutionFlowRuntime::new(
        AgentExecutionFlowRuntimeDependencies {
            agents: agents.clone(),
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
    let prepared = PreparedAgentExecution {
        organization_id,
        execution_id: execution.id,
        binding: binding.clone(),
        runtime_started_at_ms: None,
    };
    let dispatched = match super::super::runtime::dispatch(
        &flow_runtime,
        &execution.operation_id.to_string(),
        DispatchInput {
            prepared: Box::new(prepared),
        },
    )
    .await
    .expect("dispatch provider start")
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
    let initial_observed_at = canonical_timestamp(Utc::now());
    let initial_started_at_ms =
        u64::try_from(binding.bound_at().timestamp_millis()).expect("Runtime start time");
    record_running_observation(
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        &binding,
        initial_started_at_ms,
        initial_observed_at,
    )
    .await;

    let identity = binding.provider_identity().expect("provider identity");
    let event_at_ms = initial_started_at_ms + 1;
    let approval_page = AgentProviderEventPageV1 {
        schema: AgentProviderEventPageV1::SCHEMA.into(),
        identity,
        after_event_sequence: None,
        first_available_sequence: Some(0),
        source_first_sequence: Some(0),
        source_last_sequence: Some(0),
        source_event_count: 1,
        latest_sequence_exclusive: 1,
        next_after_event_sequence: Some(0),
        state: AgentProviderRunStateV1::AwaitingApproval,
        observed_at_ms: event_at_ms,
        retention_gap: false,
        has_more: false,
        terminal_failure: None,
        events: vec![AgentProviderEventRecordV1 {
            sequence: 0,
            occurred_at_ms: event_at_ms,
            event: AgentProviderSemanticEventV1::ToolRequest {
                call_id: "publish-restart".into(),
                tool,
                request: AgentProviderToolPayloadIdentityV1 {
                    digest: format!("sha256:{}", "f".repeat(64)),
                    size_bytes: 128,
                    media_type: "application/json".into(),
                },
            },
        }],
    };
    let approval_batch = NodeAgentProviderEventBatchV1 {
        schema: NodeAgentProviderEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: node_id.as_uuid(),
        binding: binding
            .node_provider_runtime_binding(execution.id.as_uuid())
            .expect("provider Runtime binding"),
        page: approval_page,
        sent_at_ms: event_at_ms + 1,
    };
    approval_batch.validate().expect("approval event batch");
    let approval_accepted_at = DateTime::<Utc>::from_timestamp_millis(
        i64::try_from(approval_batch.sent_at_ms + 1).expect("approval acceptance time"),
    )
    .expect("approval acceptance timestamp");
    agents
        .accept_provider_event_batch(
            AcceptAgentProviderEventBatchWrite::new(
                organization_id,
                node_id,
                approval_batch,
                approval_accepted_at,
            )
            .expect("approval event write"),
        )
        .await
        .expect("accept approval request");
    let checkpoint = agents
        .find_active_checkpoint(organization_id, execution.id)
        .await
        .expect("find active approval")
        .expect("active approval");
    assert_eq!(checkpoint.status, AgentApprovalCheckpointStatus::Pending);

    let dispatched = match super::super::runtime::observe(
        &flow_runtime,
        &execution.operation_id.to_string(),
        ObserveInput {
            dispatched: Box::new(dispatched),
        },
    )
    .await
    .expect("record provider process")
    {
        ObserveOutput::Pending {
            dispatched: Some(dispatched),
            ..
        } => *dispatched,
        _ => panic!("provider process identity must be persisted"),
    };
    assert_eq!(
        dispatched.prepared.runtime_started_at_ms,
        Some(initial_started_at_ms)
    );

    let restarted_observed_at =
        canonical_timestamp(Utc::now().max(initial_observed_at + Duration::milliseconds(2)));
    let restarted_at_ms = u64::try_from(restarted_observed_at.timestamp_millis())
        .expect("restarted Runtime timestamp")
        .saturating_sub(1);
    assert_ne!(restarted_at_ms, initial_started_at_ms);
    record_running_observation(
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        &binding,
        restarted_at_ms,
        restarted_observed_at,
    )
    .await;

    match super::super::runtime::observe(
        &flow_runtime,
        &execution.operation_id.to_string(),
        ObserveInput {
            dispatched: Box::new(dispatched),
        },
    )
    .await
    .expect("fail closed after provider restart")
    {
        ObserveOutput::Terminal { .. } => {}
        _ => panic!("provider restart with pending approval must fail the execution"),
    }
    let closed = agents
        .find_checkpoint(organization_id, checkpoint.id)
        .await
        .expect("find closed approval")
        .expect("closed approval");
    assert_eq!(closed.status, AgentApprovalCheckpointStatus::Cancelled);
    let failed = agents
        .find_execution(organization_id, execution.id)
        .await
        .expect("find failed execution")
        .expect("failed execution");
    assert_eq!(
        failed.status,
        crate::modules::agents::domain::AgentExecutionStatus::Failed
    );

    let now = canonical_timestamp(Utc::now());
    let lease = nodes
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence: start.sequence,
                max_commands: 1,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            now,
            now + Duration::seconds(10),
        )
        .await
        .expect("inspect commands after failed approval");
    assert!(
        lease.commands.is_empty(),
        "provider restart must not enqueue Recover or Resume while approval is unresolved"
    );
}

#[tokio::test]
async fn provider_cannot_enter_approval_without_an_exact_checkpoint() {
    let requested_at = canonical_timestamp(Utc::now() - Duration::seconds(5));
    let organization_id = OrganizationId::new();
    let node_id = NodeId::new();
    let profile = AgentProviderProfile::parse_acl(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/a1.3/reference-echo-provider-profile.acl"
    )))
    .expect("reference provider profile");
    let provider =
        AgentProviderProfileBinding::from_profile(&profile).expect("reference provider binding");
    let tool = HarnessToolBindingV1 {
        name: "workspace.publish".into(),
        revision: "1.0.0".into(),
        contract_digest: format!("sha256:{}", "e".repeat(64)),
        approval_required: true,
    };
    let agents = InMemoryAgentRepository::new();
    let (execution, binding) = prepare_bound_execution_with_provider_and_tools(
        &agents,
        organization_id,
        node_id,
        requested_at,
        provider,
        vec![tool],
    )
    .await;
    let observed_at_ms = u64::try_from(binding.bound_at().timestamp_millis())
        .expect("provider observation time")
        + 1;
    let page = AgentProviderEventPageV1 {
        schema: AgentProviderEventPageV1::SCHEMA.into(),
        identity: binding.provider_identity().expect("provider identity"),
        after_event_sequence: None,
        first_available_sequence: None,
        source_first_sequence: None,
        source_last_sequence: None,
        source_event_count: 0,
        latest_sequence_exclusive: 0,
        next_after_event_sequence: None,
        state: AgentProviderRunStateV1::AwaitingApproval,
        observed_at_ms,
        retention_gap: false,
        has_more: false,
        terminal_failure: None,
        events: Vec::new(),
    };
    page.validate_for(&profile)
        .expect("structurally valid empty paused page");
    let batch = NodeAgentProviderEventBatchV1 {
        schema: NodeAgentProviderEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: node_id.as_uuid(),
        binding: binding
            .node_provider_runtime_binding(execution.id.as_uuid())
            .expect("provider Runtime binding"),
        page,
        sent_at_ms: observed_at_ms + 1,
    };
    batch.validate().expect("provider event batch");
    let accepted_at = DateTime::<Utc>::from_timestamp_millis(
        i64::try_from(batch.sent_at_ms + 1).expect("provider acceptance time"),
    )
    .expect("provider acceptance timestamp");
    let error = agents
        .accept_provider_event_batch(
            AcceptAgentProviderEventBatchWrite::new(organization_id, node_id, batch, accepted_at)
                .expect("provider event write"),
        )
        .await
        .expect_err("approval without a checkpoint must fail closed");
    assert_eq!(
        error,
        crate::modules::shared_kernel::domain::RepositoryError::Conflict(
            "Agent provider entered approval without an exact checkpoint".into()
        )
    );
    let current = agents
        .find_execution(organization_id, execution.id)
        .await
        .expect("find unchanged execution")
        .expect("unchanged execution");
    assert_eq!(
        current.status,
        crate::modules::agents::domain::AgentExecutionStatus::Pending
    );
    assert!(agents
        .find_active_checkpoint(organization_id, execution.id)
        .await
        .expect("find active checkpoint")
        .is_none());
}
