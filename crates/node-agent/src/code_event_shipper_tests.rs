use super::*;
use crate::code_harness::CodeHarnessTransport;
use a3s_cloud_contracts::{
    AgentProtocolCommandReceiptV1, AgentProtocolCommandV1, AgentProtocolEventPageV1,
    AgentProtocolEventRecordV1, AgentProtocolRunIdentityV1, NodeCommandAck, NodeCommandAckReceipt,
    NodeCommandLeaseResponse, NodeGatewayAck, NodeGatewayAckReceipt, NodeLogChunkBatch,
    NodeLogChunkReceipt, NodeObservationBatchV2, NodeObservationReceipt, NodeResourceInventory,
    NodeResourceInventoryReceipt, AGENT_PROTOCOL_V1,
};
use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeCapabilities, RuntimeEvidence,
    RuntimeExecRequest, RuntimeExecResult, RuntimeInspection, RuntimeLogChunk, RuntimeLogQuery,
    RuntimeObservation, RuntimeRemoval, RuntimeServiceEndpoint, RuntimeUnitClass, RuntimeUnitState,
};
use a3s_runtime::{RuntimeError, RuntimeResult};
use async_trait::async_trait;
use serde_json::json;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;

struct EventRuntime {
    calls: AtomicUsize,
    observation: RuntimeObservation,
}

#[async_trait]
impl RuntimeClient for EventRuntime {
    async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        Err(RuntimeError::Protocol(
            "unexpected capabilities call".into(),
        ))
    }

    async fn apply(&self, _request: &RuntimeApplyRequest) -> RuntimeResult<RuntimeObservation> {
        Err(RuntimeError::Protocol("unexpected apply call".into()))
    }

    async fn inspect(&self, unit_id: &str) -> RuntimeResult<RuntimeInspection> {
        assert_eq!(unit_id, self.observation.unit_id);
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(RuntimeInspection::Found {
            schema: RuntimeInspection::SCHEMA.into(),
            observation: Box::new(self.observation.clone()),
        })
    }

    async fn stop(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeInspection> {
        Err(RuntimeError::Protocol("unexpected stop call".into()))
    }

    async fn remove(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeRemoval> {
        Err(RuntimeError::Protocol("unexpected remove call".into()))
    }

    async fn logs(&self, _query: &RuntimeLogQuery) -> RuntimeResult<Vec<RuntimeLogChunk>> {
        Err(RuntimeError::Protocol("unexpected logs call".into()))
    }

    async fn exec(&self, _request: &RuntimeExecRequest) -> RuntimeResult<RuntimeExecResult> {
        Err(RuntimeError::Protocol("unexpected exec call".into()))
    }
}

struct PageHarness {
    calls: AtomicUsize,
    requests: Mutex<Vec<AgentProtocolEventPageRequestV1>>,
    pages: Mutex<VecDeque<AgentProtocolEventPageV1>>,
    endpoint: RuntimeServiceEndpoint,
}

#[async_trait]
impl CodeHarnessTransport for PageHarness {
    async fn send_command(
        &self,
        _endpoint: &RuntimeServiceEndpoint,
        _command: &AgentProtocolCommandV1,
        _timeout: Duration,
    ) -> Result<AgentProtocolCommandReceiptV1, CodeHarnessError> {
        Err(CodeHarnessError::Invalid(
            "unexpected Code command request".into(),
        ))
    }

    async fn event_page(
        &self,
        endpoint: &RuntimeServiceEndpoint,
        request: &AgentProtocolEventPageRequestV1,
        timeout: Duration,
    ) -> Result<AgentProtocolEventPageV1, CodeHarnessError> {
        assert_eq!(endpoint, &self.endpoint);
        assert!(!timeout.is_zero());
        request
            .validate()
            .map_err(|error| CodeHarnessError::Invalid(error.code().into()))?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().await.push(request.clone());
        let page = self.pages.lock().await.pop_front().ok_or_else(|| {
            CodeHarnessError::Invalid("unexpected additional Code event page request".into())
        })?;
        assert_eq!(page.identity, request.identity);
        assert_eq!(page.after_event_sequence, request.after_event_sequence);
        Ok(page)
    }
}

struct EventTransport {
    failures: AtomicUsize,
    batches: Mutex<Vec<NodeCodeAgentEventBatchV1>>,
}

impl EventTransport {
    fn new(failures: usize) -> Self {
        Self {
            failures: AtomicUsize::new(failures),
            batches: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl NodeControlTransport for EventTransport {
    async fn lease(
        &self,
        _after_sequence: u64,
        _max_commands: u16,
        _wait_ms: u64,
    ) -> Result<NodeCommandLeaseResponse, NodeControlClientError> {
        Err(NodeControlClientError::Invalid("unexpected lease".into()))
    }

    async fn acknowledge(
        &self,
        _acknowledgement: &NodeCommandAck,
    ) -> Result<NodeCommandAckReceipt, NodeControlClientError> {
        Err(NodeControlClientError::Invalid(
            "unexpected acknowledgement".into(),
        ))
    }

    async fn record_observations(
        &self,
        _batch: &NodeObservationBatchV2,
    ) -> Result<NodeObservationReceipt, NodeControlClientError> {
        Err(NodeControlClientError::Invalid(
            "unexpected observation batch".into(),
        ))
    }

    async fn report_resource_inventory(
        &self,
        _inventory: &NodeResourceInventory,
    ) -> Result<NodeResourceInventoryReceipt, NodeControlClientError> {
        Err(NodeControlClientError::Invalid(
            "unexpected resource inventory".into(),
        ))
    }

    async fn record_log_chunks(
        &self,
        _batch: &NodeLogChunkBatch,
    ) -> Result<NodeLogChunkReceipt, NodeControlClientError> {
        Err(NodeControlClientError::Invalid(
            "unexpected log upload".into(),
        ))
    }

    async fn record_code_agent_events(
        &self,
        batch: &NodeCodeAgentEventBatchV1,
    ) -> Result<NodeCodeAgentEventReceiptV1, NodeControlClientError> {
        batch.validate().map_err(NodeControlClientError::Invalid)?;
        self.batches.lock().await.push(batch.clone());
        if self.failures.load(Ordering::SeqCst) > 0 {
            self.failures.fetch_sub(1, Ordering::SeqCst);
            return Err(NodeControlClientError::Transport(
                "injected Code event upload interruption".into(),
            ));
        }
        Ok(NodeCodeAgentEventReceiptV1 {
            schema: NodeCodeAgentEventReceiptV1::SCHEMA.into(),
            batch_id: batch.batch_id,
            node_id: batch.node_id,
            execution_id: batch.binding.execution_id,
            identity: batch.page.identity.clone(),
            page_digest: batch
                .page
                .digest()
                .map_err(|error| NodeControlClientError::Invalid(error.to_string()))?,
            accepted_after_event_sequence: batch.page.next_after_event_sequence,
            accepted_state: batch.page.state,
            accepted_events: u16::try_from(batch.page.events.len())
                .map_err(|error| NodeControlClientError::Invalid(error.to_string()))?,
            accepted_at_ms: batch.sent_at_ms,
            replayed: self
                .batches
                .lock()
                .await
                .iter()
                .filter(|candidate| candidate.batch_id == batch.batch_id)
                .count()
                > 1,
        })
    }

    async fn record_gateway_acknowledgement(
        &self,
        _acknowledgement: &NodeGatewayAck,
    ) -> Result<NodeGatewayAckReceipt, NodeControlClientError> {
        Err(NodeControlClientError::Invalid(
            "unexpected Gateway acknowledgement".into(),
        ))
    }
}

fn binding() -> NodeCodeAgentRuntimeBindingV1 {
    NodeCodeAgentRuntimeBindingV1 {
        schema: NodeCodeAgentRuntimeBindingV1::SCHEMA.into(),
        execution_id: Uuid::now_v7(),
        workload_id: Uuid::now_v7(),
        workload_revision_id: Uuid::now_v7(),
        deployment_id: Uuid::now_v7(),
        replica_id: Uuid::now_v7(),
        runtime_unit_id: "agent-workload:revision:3".into(),
        runtime_generation: 3,
        runtime_spec_digest: format!("sha256:{}", "b".repeat(64)),
        service_port_name: "agent".into(),
        code_run_identity: AgentProtocolRunIdentityV1 {
            schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
            protocol: AGENT_PROTOCOL_V1.into(),
            agent_release_identity: format!("sha256:{}", "a".repeat(64)),
            session_id: "conversation-1".into(),
            run_id: "execution-1-attempt-1".into(),
        },
    }
}

fn runtime_fixture(
    binding: &NodeCodeAgentRuntimeBindingV1,
) -> (RuntimeObservation, RuntimeServiceEndpoint) {
    let endpoint = RuntimeServiceEndpoint::node_local_tcp(&binding.service_port_name, 49_152)
        .expect("node-local Code endpoint");
    let mut claims = BTreeMap::new();
    endpoint
        .insert_claim(&mut claims)
        .expect("Runtime endpoint claim");
    let observed_at_ms = now_ms();
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: binding.runtime_unit_id.clone(),
        generation: binding.runtime_generation,
        spec_digest: binding.runtime_spec_digest.clone(),
        class: RuntimeUnitClass::Service,
        state: RuntimeUnitState::Running,
        provider_resource_id: Some("provider-agent-service".into()),
        provider_build: Some("a3s-box-test".into()),
        observed_at_ms,
        started_at_ms: Some(observed_at_ms.saturating_sub(1)),
        finished_at_ms: None,
        health: None,
        outputs: Vec::new(),
        usage: None,
        evidence: Some(RuntimeEvidence {
            provider_build: "a3s-box-test".into(),
            spec_digest: binding.runtime_spec_digest.clone(),
            semantics_profile_digest: None,
            claims,
        }),
        provider_attestation: None,
        failure: None,
    };
    observation.validate().expect("Runtime observation");
    (observation, endpoint)
}

fn event(
    identity: &AgentProtocolRunIdentityV1,
    sequence: u64,
    occurred_at_ms: u64,
) -> AgentProtocolEventRecordV1 {
    serde_json::from_value(json!({
        "sequence": sequence,
        "occurred_at_ms": occurred_at_ms,
        "event": {
            "version": 1,
            "type": "text_delta",
            "payload": { "text": format!("event-{sequence}") },
            "metadata": {
                "session_id": identity.session_id.as_str(),
                "run_id": identity.run_id.as_str(),
                "sequence": sequence,
                "timestamp_ms": occurred_at_ms
            }
        }
    }))
    .expect("Code event record")
}

#[allow(clippy::too_many_arguments)]
fn page(
    identity: &AgentProtocolRunIdentityV1,
    after_event_sequence: Option<u64>,
    latest_sequence_exclusive: u64,
    state: AgentProtocolRunStateV1,
    observed_at_ms: u64,
    has_more: bool,
    events: Vec<AgentProtocolEventRecordV1>,
) -> AgentProtocolEventPageV1 {
    let page = AgentProtocolEventPageV1 {
        schema: AgentProtocolEventPageV1::SCHEMA.into(),
        identity: identity.clone(),
        after_event_sequence,
        first_available_sequence: (latest_sequence_exclusive > 0).then_some(0),
        latest_sequence_exclusive,
        next_after_event_sequence: events
            .last()
            .map(|event| event.sequence)
            .or(after_event_sequence),
        state,
        observed_at_ms,
        retention_gap: false,
        has_more,
        events,
    };
    page.validate().expect("Code event page");
    page
}

fn shipper(
    root: &std::path::Path,
    node_id: Uuid,
    runtime: Arc<EventRuntime>,
    harness: Arc<PageHarness>,
    transport: Arc<EventTransport>,
) -> CodeEventShipper {
    let runtime: Arc<dyn RuntimeClient> = runtime;
    let harness: SharedCodeHarnessTransport = harness;
    let transport: Arc<dyn NodeControlTransport> = transport;
    CodeEventShipper::new(
        node_id,
        runtime,
        harness,
        transport,
        root.to_owned(),
        Duration::from_secs(1),
    )
    .expect("Code event shipper")
}

fn now_ms() -> u64 {
    u64::try_from(Utc::now().timestamp_millis()).expect("current timestamp")
}

#[test]
fn cloud_code_adapters_cannot_own_a_second_run_lifecycle() {
    let transport = include_str!("code_harness.rs");
    assert!(transport.contains("AGENT_PROTOCOL_COMMAND_HTTP_PATH_V1"));
    assert!(transport.contains("AGENT_PROTOCOL_EVENT_PAGE_HTTP_PATH_V1"));
    for (name, source) in [
        ("Harness transport", transport),
        ("event projection", include_str!("code_event_shipper.rs")),
    ] {
        for forbidden in [
            "AgentSession",
            "AgentProtocolHost",
            "InMemoryRunStore",
            "spawn_run",
            "spawn_recovery",
            "cancel_run",
            "tokio::process",
            "std::process",
            "Command::new(",
        ] {
            assert!(
                !source.contains(forbidden),
                "Cloud {name} must delegate the run lifecycle to `a3s code harness`; found {forbidden}"
            );
        }
    }
}

#[tokio::test]
async fn pending_page_replays_after_restart_before_advancing_the_code_cursor() {
    let directory = tempfile::tempdir().expect("state directory");
    let node_id = Uuid::now_v7();
    let binding = binding();
    let (observation, endpoint) = runtime_fixture(&binding);
    let occurred_at_ms = now_ms();
    let first = page(
        &binding.code_run_identity,
        None,
        2,
        AgentProtocolRunStateV1::Executing,
        occurred_at_ms,
        true,
        vec![event(&binding.code_run_identity, 0, occurred_at_ms)],
    );
    let second = page(
        &binding.code_run_identity,
        Some(0),
        2,
        AgentProtocolRunStateV1::Completed,
        occurred_at_ms.saturating_add(1),
        false,
        vec![event(
            &binding.code_run_identity,
            1,
            occurred_at_ms.saturating_add(1),
        )],
    );
    let runtime = Arc::new(EventRuntime {
        calls: AtomicUsize::new(0),
        observation,
    });
    let harness = Arc::new(PageHarness {
        calls: AtomicUsize::new(0),
        requests: Mutex::new(Vec::new()),
        pages: Mutex::new(VecDeque::from([first, second])),
        endpoint,
    });
    let transport = Arc::new(EventTransport::new(1));
    let first_shipper = shipper(
        directory.path(),
        node_id,
        Arc::clone(&runtime),
        Arc::clone(&harness),
        Arc::clone(&transport),
    );

    let interrupted = first_shipper
        .ship_once(std::slice::from_ref(&binding))
        .await;
    assert!(matches!(
        interrupted,
        Err(CodeEventShippingError::ControlPlane(
            NodeControlClientError::Transport(_)
        ))
    ));
    assert_eq!(harness.calls.load(Ordering::SeqCst), 1);

    let restarted = shipper(
        directory.path(),
        node_id,
        Arc::clone(&runtime),
        Arc::clone(&harness),
        Arc::clone(&transport),
    );
    assert!(restarted
        .ship_once(std::slice::from_ref(&binding))
        .await
        .expect("replay pending page"));
    assert_eq!(harness.calls.load(Ordering::SeqCst), 1);
    assert!(restarted
        .ship_once(std::slice::from_ref(&binding))
        .await
        .expect("ship next page"));
    assert!(!restarted
        .ship_once(&[binding])
        .await
        .expect("terminal run is drained"));

    let requests = harness.requests.lock().await.clone();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].after_event_sequence, None);
    assert_eq!(requests[1].after_event_sequence, Some(0));
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 2);
    let batches = transport.batches.lock().await.clone();
    assert_eq!(batches.len(), 3);
    assert_eq!(batches[0], batches[1]);
    assert_eq!(batches[0].batch_id, batches[1].batch_id);
    assert_ne!(batches[1].batch_id, batches[2].batch_id);
    assert_eq!(batches[2].page.after_event_sequence, Some(0));
}

#[tokio::test]
async fn empty_page_projects_a_state_change_once_without_fabricating_events() {
    let directory = tempfile::tempdir().expect("state directory");
    let node_id = Uuid::now_v7();
    let binding = binding();
    let (observation, endpoint) = runtime_fixture(&binding);
    let observed_at_ms = now_ms();
    let first = page(
        &binding.code_run_identity,
        None,
        0,
        AgentProtocolRunStateV1::Planning,
        observed_at_ms,
        false,
        Vec::new(),
    );
    let unchanged = page(
        &binding.code_run_identity,
        None,
        0,
        AgentProtocolRunStateV1::Planning,
        observed_at_ms.saturating_add(1),
        false,
        Vec::new(),
    );
    let runtime = Arc::new(EventRuntime {
        calls: AtomicUsize::new(0),
        observation,
    });
    let harness = Arc::new(PageHarness {
        calls: AtomicUsize::new(0),
        requests: Mutex::new(Vec::new()),
        pages: Mutex::new(VecDeque::from([first, unchanged])),
        endpoint,
    });
    let transport = Arc::new(EventTransport::new(0));
    let shipper = shipper(
        directory.path(),
        node_id,
        runtime,
        harness,
        Arc::clone(&transport),
    );

    assert!(shipper
        .ship_once(std::slice::from_ref(&binding))
        .await
        .expect("ship state transition"));
    assert!(!shipper
        .ship_once(&[binding])
        .await
        .expect("unchanged empty page is idle"));
    let batches = transport.batches.lock().await.clone();
    assert_eq!(batches.len(), 1);
    assert!(batches[0].page.events.is_empty());
    assert_eq!(batches[0].page.state, AgentProtocolRunStateV1::Planning);
}
