use super::super::api::PeerCertificate;
use super::{capabilities, enroll_node, NodeControlApi};
use crate::modules::agents::domain::{
    AgentCodeRunBinding, AgentConversation, AgentConversationCreated, AgentEventContent,
    AgentExecution, AgentExecutionEventDraft, AgentExecutionEventKind, AgentExecutionStarted,
    AgentExecutionStatus, AgentReleaseBinding, BindAgentCodeRunWrite, CreateAgentConversationWrite,
    IAgentRepository, StartAgentExecutionWrite,
};
use crate::modules::agents::infrastructure::InMemoryAgentRepository;
use crate::modules::artifacts::LocalNodeArtifactStore;
use crate::modules::edge::infrastructure::persistence::InMemoryEdgeRepository;
use crate::modules::edge::{EdgeGatewayAcknowledgementProjector, LocalGatewayCertificateAuthority};
use crate::modules::fleet::domain::repositories::{INodeControlRepository, INodeRepository};
use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
use crate::modules::fleet::infrastructure::{LocalCertificateAuthority, LogChunkObjectStore};
use crate::modules::secrets::infrastructure::InMemorySecretRepository;
use crate::modules::shared_kernel::domain::{
    AgentConversationId, AgentExecutionId, AssetId, AssetReleaseId, BuildRunId, DeploymentId,
    EnvironmentId, IdempotencyRequest, NodeId, OperationId, OrganizationId, ProjectId,
    Sha256Digest, WorkloadId, WorkloadReplicaId, WorkloadRevisionId,
};
use crate::modules::workloads::infrastructure::InMemoryWorkloadRepository;
use a3s_cloud_contracts::{
    AgentProtocolEventPageV1, AgentProtocolEventRecordV1, AgentProtocolRunIdentityV1,
    AgentProtocolRunStateV1, NodeCodeAgentEventBatchV1, NodeCodeAgentEventReceiptV1,
    AGENT_PROTOCOL_V1,
};
use a3s_cloud_node_agent::FileNodeIdentityStore;
use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn authenticated_node_projects_code_semantics_and_replays_one_receipt() {
    let directory = tempfile::tempdir().expect("node-control directory");
    let authority = Arc::new(
        LocalCertificateAuthority::load_or_create(directory.path().join("node-ca"))
            .expect("local CA"),
    );
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let agents = Arc::new(InMemoryAgentRepository::new());
    let identity_store = FileNodeIdentityStore::new(directory.path().join("node-identity"));
    let (organization_id, enrolled_identity) =
        enroll_node(Arc::clone(&nodes), Arc::clone(&authority), &identity_store).await;
    let node_id = enrolled_identity.response.node_id;
    let (execution, batch) =
        prepare_execution(agents.as_ref(), organization_id, NodeId::from_uuid(node_id)).await;

    let commands: Arc<dyn INodeControlRepository> = nodes.clone();
    let node_repository: Arc<dyn INodeRepository> = nodes.clone();
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let api = NodeControlApi::new(
        node_repository,
        commands,
        agents.clone(),
        Arc::new(
            LocalNodeArtifactStore::new(directory.path().join("artifacts"), 1024 * 1024)
                .expect("artifact store"),
        ),
        Arc::new(EdgeGatewayAcknowledgementProjector::new(edge.clone())),
        edge,
        Arc::new(
            LocalGatewayCertificateAuthority::load_or_create(directory.path().join("gateway-ca"))
                .expect("Gateway CA"),
        ),
        Arc::new(LogChunkObjectStore::local(directory.path()).expect("log object store")),
        authority,
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(
            crate::modules::fleet::infrastructure::LocalKeyEncryptionService::load_or_create(
                directory.path().join("secret-key"),
            )
            .expect("Secret encryption"),
        ),
        Duration::days(30),
        Duration::hours(1),
        Duration::minutes(5),
        Duration::seconds(30),
        StdDuration::from_millis(100),
        StdDuration::from_millis(5),
        1024 * 1024,
        StdDuration::from_secs(1),
        StdDuration::from_secs(5),
    )
    .expect("node-control API");
    let certificate = nodes
        .find_active_certificate(organization_id, NodeId::from_uuid(node_id))
        .await
        .expect("active node certificate");
    let router = api.router().layer(axum::Extension(PeerCertificate {
        fingerprint: certificate.fingerprint,
    }));

    let first = post_batch(&router, &batch).await;
    assert_eq!(first.status(), axum::http::StatusCode::OK);
    let first = decode_receipt(first).await;
    first.validate_for(&batch).expect("exact event receipt");
    assert!(!first.replayed);

    let replay = post_batch(&router, &batch).await;
    assert_eq!(replay.status(), axum::http::StatusCode::OK);
    let replay = decode_receipt(replay).await;
    replay.validate_for(&batch).expect("exact replay receipt");
    assert!(replay.replayed);
    assert_eq!(replay.batch_id, first.batch_id);
    assert_eq!(replay.page_digest, first.page_digest);

    let stored = agents
        .find_execution(organization_id, execution.id)
        .await
        .expect("read execution")
        .expect("stored execution");
    assert_eq!(stored.status, AgentExecutionStatus::Succeeded);
    assert_eq!(
        stored
            .code
            .as_ref()
            .expect("Code run binding")
            .accepted_after_event_sequence(),
        Some(0)
    );
    let events = agents
        .list_events(organization_id, execution.conversation_id, None, 10)
        .await
        .expect("list projected events");
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].kind, AgentExecutionEventKind::ExecutionRequested);
    assert_eq!(events[1].kind, AgentExecutionEventKind::ModelOutput);
    assert_eq!(events[2].kind, AgentExecutionEventKind::ExecutionCompleted);
}

async fn prepare_execution(
    agents: &InMemoryAgentRepository,
    organization_id: OrganizationId,
    node_id: NodeId,
) -> (AgentExecution, NodeCodeAgentEventBatchV1) {
    let requested_at = Utc::now() - Duration::seconds(5);
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
            idempotency: idempotency("node-control-code-conversations", "create", b"create"),
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
    let execution = AgentExecution::create(
        organization_id,
        conversation.id,
        AgentExecutionId::new(),
        OperationId::new(),
        release,
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
            idempotency: idempotency("node-control-code-executions", "start", b"start"),
        })
        .await
        .expect("start execution");

    let bound_at = requested_at + Duration::seconds(1);
    let binding = AgentCodeRunBinding::new(
        node_id,
        WorkloadId::new(),
        WorkloadRevisionId::new(),
        DeploymentId::new(),
        WorkloadReplicaId::new(),
        "agent-workload:revision:1",
        1,
        Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("Runtime digest"),
        "agent",
        AgentProtocolRunIdentityV1 {
            schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
            protocol: AGENT_PROTOCOL_V1.into(),
            agent_release_identity: execution.agent.artifact_digest().as_str().into(),
            session_id: format!("conversation-{}", conversation.id),
            run_id: format!("execution-{}-attempt-1", execution.id),
        },
        bound_at,
    )
    .expect("Code run binding");
    agents
        .bind_code_run(BindAgentCodeRunWrite {
            organization_id,
            execution_id: execution.id,
            binding: binding.clone(),
        })
        .await
        .expect("bind Code run");

    let occurred_at_ms = timestamp_ms(bound_at + Duration::milliseconds(1));
    let record = event_record(binding.identity(), occurred_at_ms);
    let observed_at_ms = occurred_at_ms + 1;
    let page = AgentProtocolEventPageV1 {
        schema: AgentProtocolEventPageV1::SCHEMA.into(),
        identity: binding.identity().clone(),
        after_event_sequence: None,
        first_available_sequence: Some(0),
        latest_sequence_exclusive: 1,
        next_after_event_sequence: Some(0),
        state: AgentProtocolRunStateV1::Completed,
        observed_at_ms,
        retention_gap: false,
        has_more: false,
        events: vec![record],
    };
    page.validate().expect("Code event page");
    let batch = NodeCodeAgentEventBatchV1 {
        schema: NodeCodeAgentEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: node_id.as_uuid(),
        binding: binding.node_runtime_binding(execution.id.as_uuid()),
        page,
        sent_at_ms: observed_at_ms + 1,
    };
    batch.validate().expect("Code event batch");
    (execution, batch)
}

fn event_record(
    identity: &AgentProtocolRunIdentityV1,
    occurred_at_ms: u64,
) -> AgentProtocolEventRecordV1 {
    serde_json::from_value(serde_json::json!({
        "sequence": 0,
        "occurred_at_ms": occurred_at_ms,
        "event": {
            "version": 1,
            "type": "text_delta",
            "payload": {"text": "hello"},
            "metadata": {
                "session_id": identity.session_id.as_str(),
                "run_id": identity.run_id.as_str(),
                "sequence": 0,
                "timestamp_ms": occurred_at_ms
            }
        }
    }))
    .expect("Code event record")
}

fn idempotency(scope: &str, key: &str, body: &[u8]) -> IdempotencyRequest {
    IdempotencyRequest::new(scope, key, body).expect("idempotency")
}

fn timestamp_ms(value: DateTime<Utc>) -> u64 {
    u64::try_from(value.timestamp_millis()).expect("timestamp")
}

async fn post_batch(
    router: &axum::Router,
    batch: &NodeCodeAgentEventBatchV1,
) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/v1/node-control/code-agent-events")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(batch).expect("encode event batch"),
                ))
                .expect("Code event request"),
        )
        .await
        .expect("Code event response")
}

async fn decode_receipt(response: axum::response::Response) -> NodeCodeAgentEventReceiptV1 {
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("event receipt body");
    serde_json::from_slice(&body).expect("event receipt")
}
