use super::*;
use crate::modules::agents::domain::{
    AgentCodeRunBinding, AgentConversationCreated, AgentEventContent,
    AgentExecutionCancellationRequested, AgentExecutionEventDraft, AgentExecutionEventKind,
    AgentExecutionStarted, AgentExecutionStatus, AgentReleaseBinding,
    RequestAgentExecutionCancellationWrite,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, DeploymentId, NodeId, OperationId, Sha256Digest,
    WorkloadId, WorkloadReplicaId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{
    AgentProtocolEventPageV1, AgentProtocolEventRecordV1, AgentProtocolRunIdentityV1,
    AgentProtocolRunStateV1, NodeCodeAgentEventBatchV1, AGENT_PROTOCOL_V1,
};
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

fn idempotency(scope: &str, key: &str, body: &[u8]) -> IdempotencyRequest {
    IdempotencyRequest::new(scope, key, body).expect("idempotency")
}

fn conversation() -> AgentConversation {
    AgentConversation::create(
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        AgentConversationId::new(),
        Utc::now(),
    )
    .expect("conversation")
}

async fn create_conversation(
    repository: &InMemoryAgentRepository,
    conversation: AgentConversation,
) -> AgentConversation {
    let event = AgentConversationCreated::envelope(&conversation, Uuid::now_v7())
        .expect("conversation event");
    repository
        .create_conversation(CreateAgentConversationWrite {
            conversation,
            event,
            idempotency: idempotency("agent-conversations", "create", b"create"),
        })
        .await
        .expect("create conversation")
        .conversation
}

fn execution(conversation: &AgentConversation) -> AgentExecution {
    let digest = Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest");
    let binding = AgentReleaseBinding::new(
        conversation.organization_id,
        AssetId::new(),
        AssetReleaseId::new(),
        BuildRunId::new(),
        format!("oci://registry.example/agents/demo@{digest}"),
        digest,
        "application/vnd.oci.image.manifest.v1+json",
        42,
    )
    .expect("binding");
    AgentExecution::create(
        conversation.organization_id,
        conversation.id,
        AgentExecutionId::new(),
        OperationId::new(),
        binding,
        conversation.created_at,
    )
    .expect("execution")
}

async fn start_execution(
    repository: &InMemoryAgentRepository,
    execution: AgentExecution,
) -> AgentExecutionWrite {
    let event =
        AgentExecutionStarted::envelope(&execution, Uuid::now_v7()).expect("execution event");
    repository
        .start_execution(StartAgentExecutionWrite {
            initial_event: AgentExecutionEventDraft::new(
                AgentExecutionEventKind::ExecutionRequested,
                AgentEventContent::inline_json(serde_json::json!({"prompt": "hello"}))
                    .expect("input"),
                execution.requested_at,
            )
            .expect("initial event"),
            execution,
            event,
            idempotency: idempotency("agent-executions", "start", b"start"),
        })
        .await
        .expect("start execution")
}

fn code_binding(
    execution: &AgentExecution,
    node_id: NodeId,
    bound_at: DateTime<Utc>,
) -> AgentCodeRunBinding {
    AgentCodeRunBinding::new(
        node_id,
        WorkloadId::new(),
        WorkloadRevisionId::new(),
        DeploymentId::new(),
        WorkloadReplicaId::new(),
        format!("workload:{}:revision:code", Uuid::now_v7()),
        1,
        Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("Runtime digest"),
        "code-harness",
        AgentProtocolRunIdentityV1 {
            schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
            protocol: AGENT_PROTOCOL_V1.into(),
            agent_release_identity: execution.agent.artifact_digest().as_str().into(),
            session_id: format!("conversation-{}", execution.conversation_id),
            run_id: format!("execution-{}-attempt-1", execution.id),
        },
        bound_at,
    )
    .expect("Code run binding")
}

fn event_record(
    identity: &AgentProtocolRunIdentityV1,
    sequence: u64,
    occurred_at_ms: u64,
) -> AgentProtocolEventRecordV1 {
    serde_json::from_value(serde_json::json!({
        "sequence": sequence,
        "occurred_at_ms": occurred_at_ms,
        "event": {
            "version": 1,
            "type": "text_delta",
            "payload": {"text": format!("event-{sequence}")},
            "metadata": {
                "session_id": identity.session_id,
                "run_id": identity.run_id,
                "sequence": sequence,
                "timestamp_ms": occurred_at_ms,
            }
        }
    }))
    .expect("Code event record")
}

fn event_batch(
    execution: &AgentExecution,
    binding: &AgentCodeRunBinding,
    batch_id: Uuid,
    state: AgentProtocolRunStateV1,
    event_count: u64,
) -> NodeCodeAgentEventBatchV1 {
    let bound_at_ms = u64::try_from(binding.bound_at().timestamp_millis()).expect("bound time");
    let events = (0..event_count)
        .map(|sequence| event_record(binding.identity(), sequence, bound_at_ms + sequence + 1))
        .collect::<Vec<_>>();
    let observed_at_ms = bound_at_ms + event_count + 1;
    let page = AgentProtocolEventPageV1 {
        schema: AgentProtocolEventPageV1::SCHEMA.into(),
        identity: binding.identity().clone(),
        after_event_sequence: None,
        first_available_sequence: (!events.is_empty()).then_some(0),
        latest_sequence_exclusive: event_count,
        next_after_event_sequence: events.last().map(|event| event.sequence),
        state,
        observed_at_ms,
        retention_gap: false,
        has_more: false,
        events,
    };
    page.validate().expect("Code event page");
    NodeCodeAgentEventBatchV1 {
        schema: NodeCodeAgentEventBatchV1::SCHEMA.into(),
        batch_id,
        node_id: binding.node_id().as_uuid(),
        binding: binding.node_runtime_binding(execution.id.as_uuid()),
        page,
        sent_at_ms: observed_at_ms + 1,
    }
}

fn accepted_at(batch: &NodeCodeAgentEventBatchV1) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(i64::try_from(batch.sent_at_ms + 1).expect("accepted time"))
        .expect("accepted timestamp")
}

#[tokio::test]
async fn conversation_and_execution_writes_replay_exactly_once() {
    let repository = InMemoryAgentRepository::new();
    let conversation = conversation();
    let event = AgentConversationCreated::envelope(&conversation, Uuid::now_v7())
        .expect("conversation event");
    let create = || CreateAgentConversationWrite {
        conversation: conversation.clone(),
        event: event.clone(),
        idempotency: idempotency("agent-conversations", "create", b"same"),
    };
    assert!(
        !repository
            .create_conversation(create())
            .await
            .expect("create")
            .replayed
    );
    assert!(
        repository
            .create_conversation(create())
            .await
            .expect("replay")
            .replayed
    );
    let mut changed = create();
    changed.idempotency = idempotency("agent-conversations", "create", b"changed");
    assert!(matches!(
        repository.create_conversation(changed).await,
        Err(RepositoryError::IdempotencyConflict)
    ));

    let execution = execution(&conversation);
    let write = start_execution(&repository, execution.clone()).await;
    assert_eq!(write.conversation.last_event_sequence, 1);
    assert_eq!(write.execution, execution);
    assert_eq!(repository.outbox_events().await.len(), 2);
}

#[tokio::test]
async fn cancellation_transition_is_idempotent_and_remains_operation_reconcilable() {
    let repository = InMemoryAgentRepository::new();
    let conversation = create_conversation(&repository, conversation()).await;
    let execution = execution(&conversation);
    start_execution(&repository, execution.clone()).await;
    let expected_version = execution.aggregate_version;
    let cancelled_at = execution.requested_at + Duration::seconds(1);
    let mut cancelling = execution.clone();
    cancelling
        .request_cancellation(cancelled_at)
        .expect("request cancellation");
    let event = AgentExecutionCancellationRequested::envelope(&cancelling, Uuid::now_v7())
        .expect("cancellation event");
    let write = || RequestAgentExecutionCancellationWrite {
        execution: cancelling.clone(),
        expected_version,
        event: event.clone(),
        idempotency: idempotency("agent-execution-cancel", "cancel", b"cancel"),
    };

    let first = repository
        .request_cancellation(write())
        .await
        .expect("cancel execution");
    assert!(!first.replayed);
    assert_eq!(first.execution.status, AgentExecutionStatus::Cancelling);
    assert_eq!(
        first.execution.cancellation_requested_at,
        Some(cancelled_at)
    );
    let replay = repository
        .request_cancellation(write())
        .await
        .expect("replay cancellation");
    assert!(replay.replayed);
    assert_eq!(replay.execution, first.execution);
    assert_eq!(repository.outbox_events().await.len(), 3);

    let pending = repository
        .pending_operation_starts(10)
        .await
        .expect("pending operations");
    assert_eq!(pending, vec![first.execution]);
}

#[tokio::test]
async fn concurrent_event_appends_commit_one_contiguous_conversation_sequence() {
    let repository = std::sync::Arc::new(InMemoryAgentRepository::new());
    let conversation = create_conversation(&repository, conversation()).await;
    let execution = execution(&conversation);
    start_execution(&repository, execution.clone()).await;

    let mut tasks = Vec::new();
    for index in 0..16_u8 {
        let repository = std::sync::Arc::clone(&repository);
        let execution = execution.clone();
        tasks.push(tokio::spawn(async move {
            repository
                .append_events(AppendAgentExecutionEventsWrite {
                    organization_id: execution.organization_id,
                    conversation_id: execution.conversation_id,
                    execution_id: execution.id,
                    events: vec![AgentExecutionEventDraft::new(
                        AgentExecutionEventKind::ModelOutput,
                        AgentEventContent::inline_json(serde_json::json!({"index": index}))
                            .expect("content"),
                        execution.requested_at,
                    )
                    .expect("event")],
                    idempotency: idempotency("agent-events", &format!("append-{index}"), &[index]),
                })
                .await
                .expect("append")
        }));
    }
    for task in tasks {
        task.await.expect("join");
    }

    let events = repository
        .list_events(conversation.organization_id, conversation.id, None, 100)
        .await
        .expect("events");
    assert_eq!(events.len(), 17);
    assert_eq!(
        events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        (1..=17).collect::<Vec<_>>()
    );
    let current = repository
        .find_conversation(conversation.organization_id, conversation.id)
        .await
        .expect("find")
        .expect("conversation");
    assert_eq!(current.last_event_sequence, 17);
}

#[tokio::test]
async fn event_append_replays_the_exact_committed_range() {
    let repository = InMemoryAgentRepository::new();
    let conversation = create_conversation(&repository, conversation()).await;
    let execution = execution(&conversation);
    start_execution(&repository, execution.clone()).await;
    let append = || AppendAgentExecutionEventsWrite {
        organization_id: execution.organization_id,
        conversation_id: execution.conversation_id,
        execution_id: execution.id,
        events: vec![AgentExecutionEventDraft::new(
            AgentExecutionEventKind::ExecutionCompleted,
            AgentEventContent::inline_json(serde_json::json!({})).expect("content"),
            execution.requested_at,
        )
        .expect("event")],
        idempotency: idempotency("agent-events", "complete", b"same"),
    };
    let committed = repository.append_events(append()).await.expect("append");
    assert!(!committed.replayed);
    assert_eq!(committed.events[0].sequence, 2);
    let replay = repository.append_events(append()).await.expect("replay");
    assert!(replay.replayed);
    assert_eq!(replay.events, committed.events);
    assert_eq!(replay.execution.status.as_str(), "succeeded");
}

#[tokio::test]
async fn code_event_batch_projects_semantics_and_replays_one_receipt() {
    let repository = InMemoryAgentRepository::new();
    let conversation = create_conversation(&repository, conversation()).await;
    let execution = execution(&conversation);
    start_execution(&repository, execution.clone()).await;
    let node_id = NodeId::new();
    let binding = code_binding(
        &execution,
        node_id,
        execution.requested_at + Duration::milliseconds(1),
    );
    let bound = repository
        .bind_code_run(BindAgentCodeRunWrite {
            organization_id: execution.organization_id,
            execution_id: execution.id,
            binding: binding.clone(),
        })
        .await
        .expect("bind Code run");
    assert!(!bound.replayed);

    let batch = event_batch(
        &execution,
        &binding,
        Uuid::now_v7(),
        AgentProtocolRunStateV1::Completed,
        2,
    );
    let write = || {
        AcceptAgentCodeEventBatchWrite::new(
            execution.organization_id,
            node_id,
            batch.clone(),
            accepted_at(&batch),
        )
        .expect("event write")
    };
    let receipt = repository
        .accept_code_event_batch(write())
        .await
        .expect("accept Code events");
    assert!(!receipt.replayed);
    assert_eq!(receipt.accepted_events, 2);
    assert_eq!(receipt.accepted_after_event_sequence, Some(1));
    assert_eq!(receipt.accepted_state, AgentProtocolRunStateV1::Completed);

    let events = repository
        .list_events(
            execution.organization_id,
            execution.conversation_id,
            None,
            10,
        )
        .await
        .expect("events");
    assert_eq!(events.len(), 4);
    assert_eq!(events[1].kind, AgentExecutionEventKind::ModelOutput);
    assert_eq!(
        events[1].content.value(),
        &batch.page.events[0].event.payload
    );
    assert_eq!(events[2].kind, AgentExecutionEventKind::ModelOutput);
    assert_eq!(events[3].kind, AgentExecutionEventKind::ExecutionCompleted);
    let current = repository
        .find_execution(execution.organization_id, execution.id)
        .await
        .expect("find execution")
        .expect("execution");
    assert_eq!(current.status.as_str(), "succeeded");
    assert_eq!(
        current
            .code
            .as_ref()
            .expect("Code binding")
            .accepted_after_event_sequence(),
        Some(1)
    );

    let replay = repository
        .accept_code_event_batch(write())
        .await
        .expect("replay Code events");
    assert!(replay.replayed);
    assert_eq!(replay.accepted_at_ms, receipt.accepted_at_ms);
    assert_eq!(
        repository
            .list_events(
                execution.organization_id,
                execution.conversation_id,
                None,
                10
            )
            .await
            .expect("events")
            .len(),
        4
    );

    let binding_replay = repository
        .bind_code_run(BindAgentCodeRunWrite {
            organization_id: execution.organization_id,
            execution_id: execution.id,
            binding,
        })
        .await
        .expect("replay Code binding after progress");
    assert!(binding_replay.replayed);
}

#[tokio::test]
async fn consecutive_code_pages_use_code_time_instead_of_control_plane_latency() {
    let repository = InMemoryAgentRepository::new();
    let conversation = create_conversation(&repository, conversation()).await;
    let execution = execution(&conversation);
    start_execution(&repository, execution.clone()).await;
    let node_id = NodeId::new();
    let binding = code_binding(
        &execution,
        node_id,
        execution.requested_at + Duration::milliseconds(1),
    );
    repository
        .bind_code_run(BindAgentCodeRunWrite {
            organization_id: execution.organization_id,
            execution_id: execution.id,
            binding: binding.clone(),
        })
        .await
        .expect("bind Code run");

    let first = event_batch(
        &execution,
        &binding,
        Uuid::now_v7(),
        AgentProtocolRunStateV1::Executing,
        1,
    );
    let first_accepted_at = DateTime::from_timestamp_millis(
        i64::try_from(first.sent_at_ms + 100).expect("first acceptance time"),
    )
    .expect("first acceptance timestamp");
    repository
        .accept_code_event_batch(
            AcceptAgentCodeEventBatchWrite::new(
                execution.organization_id,
                node_id,
                first.clone(),
                first_accepted_at,
            )
            .expect("first page write"),
        )
        .await
        .expect("accept first page");

    let occurred_at_ms = first.page.observed_at_ms + 1;
    let second_page = AgentProtocolEventPageV1 {
        schema: AgentProtocolEventPageV1::SCHEMA.into(),
        identity: binding.identity().clone(),
        after_event_sequence: Some(0),
        first_available_sequence: Some(0),
        latest_sequence_exclusive: 2,
        next_after_event_sequence: Some(1),
        state: AgentProtocolRunStateV1::Completed,
        observed_at_ms: occurred_at_ms + 1,
        retention_gap: false,
        has_more: false,
        events: vec![event_record(binding.identity(), 1, occurred_at_ms)],
    };
    second_page.validate().expect("second Code page");
    assert!(
        second_page.observed_at_ms < u64::try_from(first_accepted_at.timestamp_millis()).unwrap()
    );
    let second = NodeCodeAgentEventBatchV1 {
        schema: NodeCodeAgentEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: node_id.as_uuid(),
        binding: binding.node_runtime_binding(execution.id.as_uuid()),
        sent_at_ms: second_page.observed_at_ms + 1,
        page: second_page,
    };
    let second_accepted_at = first_accepted_at + Duration::milliseconds(1);
    repository
        .accept_code_event_batch(
            AcceptAgentCodeEventBatchWrite::new(
                execution.organization_id,
                node_id,
                second,
                second_accepted_at,
            )
            .expect("second page write"),
        )
        .await
        .expect("accept second page after delayed first receipt");
    let current = repository
        .find_execution(execution.organization_id, execution.id)
        .await
        .expect("find execution")
        .expect("execution");
    assert_eq!(current.status.as_str(), "succeeded");
    assert_eq!(
        current.code.expect("Code binding").observed_at(),
        Some(
            DateTime::from_timestamp_millis(
                i64::try_from(occurred_at_ms + 1).expect("Code observation time")
            )
            .expect("Code observation timestamp")
        )
    );
}

#[tokio::test]
async fn empty_code_page_advances_state_without_fabricating_a_semantic_event() {
    let repository = InMemoryAgentRepository::new();
    let conversation = create_conversation(&repository, conversation()).await;
    let execution = execution(&conversation);
    start_execution(&repository, execution.clone()).await;
    let node_id = NodeId::new();
    let binding = code_binding(
        &execution,
        node_id,
        execution.requested_at + Duration::milliseconds(1),
    );
    repository
        .bind_code_run(BindAgentCodeRunWrite {
            organization_id: execution.organization_id,
            execution_id: execution.id,
            binding: binding.clone(),
        })
        .await
        .expect("bind Code run");
    let batch = event_batch(
        &execution,
        &binding,
        Uuid::now_v7(),
        AgentProtocolRunStateV1::Planning,
        0,
    );
    let receipt = repository
        .accept_code_event_batch(
            AcceptAgentCodeEventBatchWrite::new(
                execution.organization_id,
                node_id,
                batch.clone(),
                accepted_at(&batch),
            )
            .expect("event write"),
        )
        .await
        .expect("accept empty page");
    assert_eq!(receipt.accepted_events, 0);
    assert_eq!(
        repository
            .list_events(
                execution.organization_id,
                execution.conversation_id,
                None,
                10
            )
            .await
            .expect("events")
            .len(),
        1
    );
    assert_eq!(
        repository
            .find_execution(execution.organization_id, execution.id)
            .await
            .expect("find execution")
            .expect("execution")
            .status
            .as_str(),
        "running"
    );
}

#[tokio::test]
async fn code_projection_does_not_compare_node_and_control_plane_clocks() {
    let repository = InMemoryAgentRepository::new();
    let conversation = create_conversation(&repository, conversation()).await;
    let execution = execution(&conversation);
    start_execution(&repository, execution.clone()).await;
    let node_id = NodeId::new();
    let bound_at = execution.requested_at + Duration::milliseconds(1);
    let binding = code_binding(&execution, node_id, bound_at);
    repository
        .bind_code_run(BindAgentCodeRunWrite {
            organization_id: execution.organization_id,
            execution_id: execution.id,
            binding: binding.clone(),
        })
        .await
        .expect("bind Code run");

    let mut batch = event_batch(
        &execution,
        &binding,
        Uuid::now_v7(),
        AgentProtocolRunStateV1::Planning,
        0,
    );
    batch.page.observed_at_ms += 10 * 60 * 1_000;
    batch.sent_at_ms = batch.page.observed_at_ms + 1;
    batch.validate().expect("skewed node batch");
    let control_plane_accepted_at = bound_at + Duration::milliseconds(1);
    assert!(
        u64::try_from(control_plane_accepted_at.timestamp_millis()).expect("control-plane time")
            < batch.sent_at_ms
    );

    repository
        .accept_code_event_batch(
            AcceptAgentCodeEventBatchWrite::new(
                execution.organization_id,
                node_id,
                batch.clone(),
                control_plane_accepted_at,
            )
            .expect("skewed page write"),
        )
        .await
        .expect("accept skewed node page");
    let current = repository
        .find_execution(execution.organization_id, execution.id)
        .await
        .expect("find execution")
        .expect("execution");
    assert_eq!(current.updated_at, control_plane_accepted_at);
    assert_eq!(
        current.code.expect("Code binding").observed_at(),
        DateTime::from_timestamp_millis(
            i64::try_from(batch.page.observed_at_ms).expect("node observation time")
        )
    );
}
