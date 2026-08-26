use super::tests::{
    code_binding, conversation, create_conversation, event_batch, execution, start_execution,
};
use super::InMemoryAgentRepository;
use crate::modules::agents::domain::{
    AcceptAgentProviderEventBatchWrite, AgentCodeRunBinding, AgentExecution,
    AgentExecutionEventKind, AgentExecutionStatus, BindAgentCodeRunWrite, IAgentRepository,
};
use crate::modules::shared_kernel::domain::{NodeId, RepositoryError};
use a3s_cloud_contracts::{
    AgentProtocolEventPageV1, AgentProtocolEventRecordV1, AgentProtocolRunStateV1,
    AgentProviderEventPageV1, AgentProviderEventRecordV1, AgentProviderRunStateV1,
    AgentProviderSemanticEventV1, NodeAgentProviderEventBatchV1,
};
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

fn provider_event_batch(
    execution: &AgentExecution,
    binding: &AgentCodeRunBinding,
    batch_id: Uuid,
    state: AgentProtocolRunStateV1,
    event_count: u64,
) -> NodeAgentProviderEventBatchV1 {
    let native = event_batch(execution, binding, batch_id, state, event_count);
    let page =
        crate::modules::agents::infrastructure::project_code_event_page(binding, &native.page)
            .expect("Agent provider event page");
    let batch = NodeAgentProviderEventBatchV1 {
        schema: NodeAgentProviderEventBatchV1::SCHEMA.into(),
        batch_id,
        node_id: binding.node_id().as_uuid(),
        binding: binding
            .node_provider_runtime_binding(execution.id.as_uuid())
            .expect("Agent provider Runtime binding"),
        page,
        sent_at_ms: native.sent_at_ms,
    };
    batch.validate().expect("Agent provider event batch");
    batch
}

fn provider_accepted_at(batch: &NodeAgentProviderEventBatchV1) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(i64::try_from(batch.sent_at_ms + 1).expect("accepted time"))
        .expect("accepted timestamp")
}

#[tokio::test]
async fn provider_event_batch_projects_semantics_and_replays_one_receipt() {
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
        .expect("bind Agent provider run");

    let batch = provider_event_batch(
        &execution,
        &binding,
        Uuid::now_v7(),
        AgentProtocolRunStateV1::Completed,
        2,
    );
    let control_plane_accepted_at = binding.bound_at();
    assert!(
        u64::try_from(control_plane_accepted_at.timestamp_millis())
            .expect("control-plane acceptance time")
            < batch.sent_at_ms
    );
    let write = || {
        AcceptAgentProviderEventBatchWrite::new(
            execution.organization_id,
            node_id,
            batch.clone(),
            control_plane_accepted_at,
        )
        .expect("Agent provider event write")
    };
    let receipt = repository
        .accept_provider_event_batch(write())
        .await
        .expect("accept Agent provider events");
    receipt.validate_for(&batch).expect("exact receipt");
    assert!(!receipt.receipt.replayed);
    assert_eq!(receipt.receipt.accepted_events, 2);
    assert_eq!(receipt.receipt.accepted_source_events, 2);
    assert_eq!(receipt.receipt.accepted_after_event_sequence, Some(1));
    assert_eq!(receipt.receipt.accepted_at_ms, batch.sent_at_ms);

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
    assert_eq!(events[2].kind, AgentExecutionEventKind::ModelOutput);
    assert_eq!(events[3].kind, AgentExecutionEventKind::ExecutionCompleted);
    let current = repository
        .find_execution(execution.organization_id, execution.id)
        .await
        .expect("find execution")
        .expect("execution");
    assert_eq!(current.status, AgentExecutionStatus::Succeeded);
    assert_eq!(
        current
            .code
            .expect("Agent provider binding")
            .accepted_after_event_sequence(),
        Some(1)
    );

    let replay = repository
        .accept_provider_event_batch(write())
        .await
        .expect("replay Agent provider events");
    replay.validate_for(&batch).expect("exact replay receipt");
    assert!(replay.receipt.replayed);
    assert_eq!(replay.receipt.page_digest, receipt.receipt.page_digest);
    assert_eq!(
        repository
            .list_events(
                execution.organization_id,
                execution.conversation_id,
                None,
                10,
            )
            .await
            .expect("events")
            .len(),
        4
    );
}

#[tokio::test]
async fn provider_event_batch_hides_other_nodes_and_rejects_runtime_identity_drift() {
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
        .expect("bind Agent provider run");

    let batch = provider_event_batch(
        &execution,
        &binding,
        Uuid::now_v7(),
        AgentProtocolRunStateV1::Executing,
        1,
    );
    let foreign_node_id = NodeId::new();
    let mut foreign_batch = batch.clone();
    foreign_batch.node_id = foreign_node_id.as_uuid();
    foreign_batch.validate().expect("foreign-node batch");
    let error = repository
        .accept_provider_event_batch(
            AcceptAgentProviderEventBatchWrite::new(
                execution.organization_id,
                foreign_node_id,
                foreign_batch.clone(),
                provider_accepted_at(&foreign_batch),
            )
            .expect("foreign-node write"),
        )
        .await
        .expect_err("foreign node must not discover the execution");
    assert_eq!(error, RepositoryError::NotFound);

    let mut drifted = batch;
    drifted.binding.runtime_generation += 1;
    drifted.validate().expect("drifted Runtime binding batch");
    let error = repository
        .accept_provider_event_batch(
            AcceptAgentProviderEventBatchWrite::new(
                execution.organization_id,
                node_id,
                drifted.clone(),
                provider_accepted_at(&drifted),
            )
            .expect("drifted Runtime binding write"),
        )
        .await
        .expect_err("Runtime identity drift must be rejected");
    assert!(matches!(error, RepositoryError::Conflict(_)));
}

#[tokio::test]
async fn provider_event_batch_persists_only_the_bounded_failure_projection() {
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
        .expect("bind Agent provider run");

    let occurred_at_ms =
        u64::try_from(binding.bound_at().timestamp_millis()).expect("bound time") + 1;
    let error_record: AgentProtocolEventRecordV1 = serde_json::from_value(serde_json::json!({
        "sequence": 0,
        "occurred_at_ms": occurred_at_ms,
        "event": {
            "version": 1,
            "type": "error",
            "payload": {"message": "provider failure\nprivate trace omitted"},
            "metadata": {
                "session_id": binding.identity().session_id,
                "run_id": binding.identity().run_id,
                "sequence": 0,
                "timestamp_ms": occurred_at_ms,
            }
        }
    }))
    .expect("native provider failure");
    let native_page = AgentProtocolEventPageV1 {
        schema: AgentProtocolEventPageV1::SCHEMA.into(),
        identity: binding.identity().clone(),
        after_event_sequence: None,
        first_available_sequence: Some(0),
        latest_sequence_exclusive: 1,
        next_after_event_sequence: Some(0),
        state: AgentProtocolRunStateV1::Failed,
        observed_at_ms: occurred_at_ms + 1,
        retention_gap: false,
        has_more: false,
        events: vec![error_record],
    };
    native_page
        .validate()
        .expect("native provider failure page");
    let page =
        crate::modules::agents::infrastructure::project_code_event_page(&binding, &native_page)
            .expect("bounded provider failure page");
    assert_eq!(
        page.terminal_failure.as_deref(),
        Some("provider failure private trace omitted")
    );
    let batch = NodeAgentProviderEventBatchV1 {
        schema: NodeAgentProviderEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: node_id.as_uuid(),
        binding: binding
            .node_provider_runtime_binding(execution.id.as_uuid())
            .expect("provider Runtime binding"),
        sent_at_ms: page.observed_at_ms + 1,
        page,
    };
    batch.validate().expect("provider failure batch");
    repository
        .accept_provider_event_batch(
            AcceptAgentProviderEventBatchWrite::new(
                execution.organization_id,
                node_id,
                batch.clone(),
                provider_accepted_at(&batch),
            )
            .expect("provider failure write"),
        )
        .await
        .expect("accept provider failure");

    let stored = repository
        .find_execution(execution.organization_id, execution.id)
        .await
        .expect("find failed execution")
        .expect("failed execution");
    assert_eq!(stored.status, AgentExecutionStatus::Failed);
    assert_eq!(
        stored.failure.as_deref(),
        Some("provider failure private trace omitted")
    );
    let events = repository
        .list_events(
            execution.organization_id,
            execution.conversation_id,
            None,
            10,
        )
        .await
        .expect("failed execution events");
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].kind, AgentExecutionEventKind::ExecutionFailed);
    assert_eq!(
        events[1].content.value(),
        &serde_json::json!({"reason": "provider failure private trace omitted"})
    );
}

#[tokio::test]
async fn provider_retention_gap_rotates_the_run_and_settles_the_predecessor_batch() {
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
        .expect("bind Agent provider run");

    let first = provider_event_batch(
        &execution,
        &binding,
        Uuid::now_v7(),
        AgentProtocolRunStateV1::Executing,
        1,
    );
    repository
        .accept_provider_event_batch(
            AcceptAgentProviderEventBatchWrite::new(
                execution.organization_id,
                node_id,
                first.clone(),
                provider_accepted_at(&first),
            )
            .expect("first provider page write"),
        )
        .await
        .expect("accept first provider page");
    let checkpoint = repository
        .find_execution(execution.organization_id, execution.id)
        .await
        .expect("find checkpoint execution")
        .expect("checkpoint execution")
        .code
        .expect("checkpoint binding");
    assert_eq!(checkpoint.accepted_after_event_sequence(), Some(0));

    let gap_observed_at_ms = first.page.observed_at_ms + 24 * 60 * 60 * 1_000;
    let gap_page = AgentProviderEventPageV1 {
        schema: AgentProviderEventPageV1::SCHEMA.into(),
        identity: checkpoint.provider_identity().expect("provider identity"),
        after_event_sequence: Some(0),
        first_available_sequence: Some(2),
        source_first_sequence: None,
        source_last_sequence: None,
        source_event_count: 0,
        latest_sequence_exclusive: 3,
        next_after_event_sequence: Some(0),
        state: AgentProviderRunStateV1::Executing,
        observed_at_ms: gap_observed_at_ms,
        retention_gap: true,
        has_more: false,
        terminal_failure: None,
        events: Vec::new(),
    };
    let gap_batch = NodeAgentProviderEventBatchV1 {
        schema: NodeAgentProviderEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: node_id.as_uuid(),
        binding: checkpoint
            .node_provider_runtime_binding(execution.id.as_uuid())
            .expect("checkpoint Runtime binding"),
        page: gap_page,
        sent_at_ms: gap_observed_at_ms + 1,
    };
    gap_batch.validate().expect("provider retention gap");
    let gap_accepted_at = provider_accepted_at(&gap_batch);
    let receipt = repository
        .accept_provider_event_batch(
            AcceptAgentProviderEventBatchWrite::new(
                execution.organization_id,
                node_id,
                gap_batch.clone(),
                gap_accepted_at,
            )
            .expect("provider retention-gap write"),
        )
        .await
        .expect("accept provider retention gap");
    assert_eq!(receipt.receipt.accepted_events, 0);
    assert_eq!(receipt.receipt.accepted_source_events, 0);

    let recovered = repository
        .find_execution(execution.organization_id, execution.id)
        .await
        .expect("find recovered execution")
        .expect("recovered execution");
    let recovered_binding = recovered.code.as_ref().expect("recovered binding");
    assert_eq!(
        recovered_binding.identity().run_id,
        AgentCodeRunBinding::recovery_run_id(execution.id, &checkpoint.identity().run_id)
    );
    assert_eq!(recovered_binding.bound_at(), gap_accepted_at);

    let stale_observed_at_ms = gap_observed_at_ms + 1;
    let stale_page = AgentProviderEventPageV1 {
        schema: AgentProviderEventPageV1::SCHEMA.into(),
        identity: checkpoint
            .provider_identity()
            .expect("checkpoint provider identity"),
        after_event_sequence: Some(0),
        first_available_sequence: Some(0),
        source_first_sequence: Some(1),
        source_last_sequence: Some(1),
        source_event_count: 1,
        latest_sequence_exclusive: 2,
        next_after_event_sequence: Some(1),
        state: AgentProviderRunStateV1::Executing,
        observed_at_ms: stale_observed_at_ms,
        retention_gap: false,
        has_more: false,
        terminal_failure: None,
        events: vec![AgentProviderEventRecordV1 {
            sequence: 1,
            occurred_at_ms: stale_observed_at_ms,
            event: AgentProviderSemanticEventV1::ModelOutput {
                text: "in-flight predecessor output".into(),
            },
        }],
    };
    let stale_batch = NodeAgentProviderEventBatchV1 {
        schema: NodeAgentProviderEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: node_id.as_uuid(),
        binding: checkpoint
            .node_provider_runtime_binding(execution.id.as_uuid())
            .expect("checkpoint Runtime binding"),
        page: stale_page,
        sent_at_ms: stale_observed_at_ms + 1,
    };
    stale_batch.validate().expect("in-flight predecessor batch");
    let stale_receipt = repository
        .accept_provider_event_batch(
            AcceptAgentProviderEventBatchWrite::new(
                execution.organization_id,
                node_id,
                stale_batch.clone(),
                provider_accepted_at(&stale_batch),
            )
            .expect("in-flight predecessor write"),
        )
        .await
        .expect("settle in-flight predecessor batch");
    assert!(!stale_receipt.receipt.replayed);
    assert_eq!(
        repository
            .find_execution(execution.organization_id, execution.id)
            .await
            .expect("find execution after predecessor settlement")
            .expect("execution after predecessor settlement"),
        recovered
    );
}
