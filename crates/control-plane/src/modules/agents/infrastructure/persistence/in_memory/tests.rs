use super::*;
use crate::modules::agents::domain::{
    AgentConversationCreated, AgentEventContent, AgentExecutionEventDraft, AgentExecutionEventKind,
    AgentExecutionStarted, AgentReleaseBinding,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, OperationId, Sha256Digest,
};
use chrono::Utc;
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
