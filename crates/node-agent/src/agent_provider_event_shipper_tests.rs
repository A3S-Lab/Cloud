use super::*;
use crate::agent_provider_harness::AgentProviderHarnessTransport;
use a3s_cloud_contracts::{
    AgentProviderCommandReceiptV1, AgentProviderCommandV1, AgentProviderEventPageV1,
    AgentProviderEventReceiptV1, AgentProviderEventRecordV1, AgentProviderProfile,
    AgentProviderRunIdentityV1, AgentProviderSemanticEventV1, NodeAgentProviderEventBatchV1,
    NodeAgentProviderEventReceiptV1, NodeCodeAgentEventBatchV1, NodeCodeAgentEventReceiptV1,
    NodeCommandAck, NodeCommandAckReceipt, NodeCommandLeaseResponse, NodeGatewayAck,
    NodeGatewayAckReceipt, NodeLogChunkBatch, NodeLogChunkReceipt, NodeObservationBatchV2,
    NodeObservationReceipt, NodeResourceInventory, NodeResourceInventoryReceipt,
};
use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeCapabilities, RuntimeEvidence,
    RuntimeExecRequest, RuntimeExecResult, RuntimeInspection, RuntimeLogChunk, RuntimeLogQuery,
    RuntimeObservation, RuntimeRemoval, RuntimeServiceEndpoint, RuntimeUnitClass, RuntimeUnitState,
};
use a3s_runtime::{RuntimeError, RuntimeResult};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;

const REFERENCE_PROFILE_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/a1.3/reference-echo-provider-profile.acl"
));

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
    pages: Mutex<Vec<AgentProviderEventPageV1>>,
    endpoint: RuntimeServiceEndpoint,
}

#[async_trait]
impl AgentProviderHarnessTransport for PageHarness {
    async fn send_command(
        &self,
        _endpoint: &RuntimeServiceEndpoint,
        _binding: &NodeAgentProviderRuntimeBindingV1,
        _command: &AgentProviderCommandV1,
        _timeout: Duration,
    ) -> Result<AgentProviderCommandReceiptV1, AgentProviderHarnessError> {
        Err(AgentProviderHarnessError::Invalid(
            "unexpected provider command request".into(),
        ))
    }

    async fn event_page(
        &self,
        endpoint: &RuntimeServiceEndpoint,
        binding: &NodeAgentProviderRuntimeBindingV1,
        request: &AgentProviderEventPageRequestV1,
        timeout: Duration,
    ) -> Result<AgentProviderEventPageV1, AgentProviderHarnessError> {
        assert_eq!(endpoint, &self.endpoint);
        assert!(!timeout.is_zero());
        let profile = binding
            .profile()
            .map_err(AgentProviderHarnessError::Invalid)?;
        request
            .validate_for(&profile)
            .map_err(AgentProviderHarnessError::Invalid)?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        let page = self.pages.lock().await.remove(0);
        assert_eq!(page.identity, request.identity);
        assert_eq!(page.after_event_sequence, request.after_event_sequence);
        Ok(page)
    }
}

struct EventTransport {
    failures: AtomicUsize,
    batches: Mutex<Vec<NodeAgentProviderEventBatchV1>>,
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
        _batch: &NodeCodeAgentEventBatchV1,
    ) -> Result<NodeCodeAgentEventReceiptV1, NodeControlClientError> {
        Err(NodeControlClientError::Invalid(
            "generic provider events must not use the Code endpoint".into(),
        ))
    }

    async fn record_agent_provider_events(
        &self,
        batch: &NodeAgentProviderEventBatchV1,
    ) -> Result<NodeAgentProviderEventReceiptV1, NodeControlClientError> {
        batch.validate().map_err(NodeControlClientError::Invalid)?;
        self.batches.lock().await.push(batch.clone());
        if self.failures.load(Ordering::SeqCst) > 0 {
            self.failures.fetch_sub(1, Ordering::SeqCst);
            return Err(NodeControlClientError::Transport(
                "injected provider event upload interruption".into(),
            ));
        }
        let profile = batch
            .binding
            .profile()
            .map_err(NodeControlClientError::Invalid)?;
        let receipt = AgentProviderEventReceiptV1::accepted(
            &profile,
            batch.batch_id,
            &batch.page,
            batch.sent_at_ms,
            self.batches
                .lock()
                .await
                .iter()
                .filter(|candidate| candidate.batch_id == batch.batch_id)
                .count()
                > 1,
        )
        .map_err(NodeControlClientError::Invalid)?;
        Ok(NodeAgentProviderEventReceiptV1 {
            schema: NodeAgentProviderEventReceiptV1::SCHEMA.into(),
            batch_id: batch.batch_id,
            node_id: batch.node_id,
            execution_id: batch.binding.execution_id,
            receipt,
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

fn profile() -> AgentProviderProfile {
    AgentProviderProfile::parse_acl(REFERENCE_PROFILE_ACL).expect("reference profile")
}

fn binding() -> NodeAgentProviderRuntimeBindingV1 {
    let profile = profile();
    NodeAgentProviderRuntimeBindingV1 {
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
        provider_run_identity: AgentProviderRunIdentityV1::new(
            profile.digest().into(),
            profile.capability_digest().into(),
            format!("sha256:{}", "a".repeat(64)),
            "conversation-1".into(),
            "execution-1-attempt-1".into(),
        )
        .expect("provider identity"),
    }
}

fn runtime_fixture(
    binding: &NodeAgentProviderRuntimeBindingV1,
) -> (RuntimeObservation, RuntimeServiceEndpoint) {
    let endpoint = RuntimeServiceEndpoint::node_local_tcp(&binding.service_port_name, 49_153)
        .expect("node-local provider endpoint");
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
        provider_resource_id: Some("provider-reference-agent".into()),
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
            identity_attachment_digest: None,
            claims,
        }),
        provider_attestation: None,
        failure: None,
    };
    observation.validate().expect("Runtime observation");
    (observation, endpoint)
}

fn page(
    identity: &AgentProviderRunIdentityV1,
    after_event_sequence: Option<u64>,
    sequence: u64,
    state: AgentProviderRunStateV1,
    observed_at_ms: u64,
    has_more: bool,
) -> AgentProviderEventPageV1 {
    let event = AgentProviderEventRecordV1 {
        sequence,
        occurred_at_ms: observed_at_ms,
        event: AgentProviderSemanticEventV1::ModelOutput {
            text: format!("event-{sequence}"),
        },
    };
    let page = AgentProviderEventPageV1 {
        schema: AgentProviderEventPageV1::SCHEMA.into(),
        identity: identity.clone(),
        after_event_sequence,
        first_available_sequence: Some(0),
        source_first_sequence: Some(sequence),
        source_last_sequence: Some(sequence),
        source_event_count: 1,
        latest_sequence_exclusive: 2,
        next_after_event_sequence: Some(sequence),
        state,
        observed_at_ms,
        retention_gap: false,
        has_more,
        terminal_failure: None,
        events: vec![event],
    };
    page.validate_for(&profile()).expect("provider page");
    page
}

fn shipper(
    root: &std::path::Path,
    node_id: Uuid,
    runtime: Arc<EventRuntime>,
    harness: Arc<PageHarness>,
    transport: Arc<EventTransport>,
) -> AgentProviderEventShipper {
    let runtime: Arc<dyn RuntimeClient> = runtime;
    let harness: SharedAgentProviderHarnessTransport = harness;
    let transport: Arc<dyn NodeControlTransport> = transport;
    AgentProviderEventShipper::new(
        node_id,
        runtime,
        harness,
        transport,
        root.to_owned(),
        Duration::from_secs(1),
    )
    .expect("provider event shipper")
}

fn now_ms() -> u64 {
    u64::try_from(Utc::now().timestamp_millis()).expect("current timestamp")
}

#[tokio::test]
async fn pending_page_replays_after_restart_before_advancing_the_provider_cursor() {
    let directory = tempfile::tempdir().expect("state directory");
    let node_id = Uuid::now_v7();
    let binding = binding();
    let (observation, endpoint) = runtime_fixture(&binding);
    let observed_at_ms = now_ms();
    let first = page(
        &binding.provider_run_identity,
        None,
        0,
        AgentProviderRunStateV1::Executing,
        observed_at_ms,
        true,
    );
    let second = page(
        &binding.provider_run_identity,
        Some(0),
        1,
        AgentProviderRunStateV1::Completed,
        observed_at_ms.saturating_add(1),
        false,
    );
    let runtime = Arc::new(EventRuntime {
        calls: AtomicUsize::new(0),
        observation,
    });
    let harness = Arc::new(PageHarness {
        calls: AtomicUsize::new(0),
        pages: Mutex::new(vec![first, second]),
        endpoint,
    });
    let transport = Arc::new(EventTransport {
        failures: AtomicUsize::new(1),
        batches: Mutex::new(Vec::new()),
    });

    let first_shipper = shipper(
        directory.path(),
        node_id,
        Arc::clone(&runtime),
        Arc::clone(&harness),
        Arc::clone(&transport),
    );
    assert!(matches!(
        first_shipper
            .ship_once(std::slice::from_ref(&binding))
            .await,
        Err(AgentProviderEventShippingError::ControlPlane(
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
        .expect("ship terminal page"));
    assert_eq!(harness.calls.load(Ordering::SeqCst), 2);
    assert!(!restarted
        .ship_once(std::slice::from_ref(&binding))
        .await
        .expect("terminal provider run drained"));

    let batches = transport.batches.lock().await;
    assert_eq!(batches.len(), 3);
    assert_eq!(batches[0].batch_id, batches[1].batch_id);
    assert_ne!(batches[1].batch_id, batches[2].batch_id);
    assert_eq!(batches[2].page.state, AgentProviderRunStateV1::Completed);
}

#[test]
fn event_shipper_rejects_a_profile_not_admitted_by_the_node_build() {
    let mut binding = binding();
    binding.provider_profile_acl = binding
        .provider_profile_acl
        .replace("revision = \"1.0.0\"", "revision = \"1.0.1\"");
    let changed = AgentProviderProfile::parse_acl(&binding.provider_profile_acl)
        .expect("canonical changed profile");
    binding.provider_profile_digest = changed.digest().into();
    binding.provider_run_identity.provider_profile_digest = changed.digest().into();
    binding.provider_run_identity.provider_capability_digest = changed.capability_digest().into();
    assert!(matches!(
        validate_bindings(&[binding]),
        Err(AgentProviderEventShippingError::Harness {
            retryable: false,
            ..
        })
    ));
}
