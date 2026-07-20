use super::*;
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandAckReceipt, NodeCommandLeaseResponse, NodeGatewayAck,
    NodeGatewayAckReceipt, NodeObservationBatch, NodeObservationReceipt,
};
use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeCapabilities, RuntimeExecRequest,
    RuntimeExecResult, RuntimeInspection, RuntimeLogChunk, RuntimeLogStream, RuntimeObservation,
    RuntimeRemoval,
};
use a3s_runtime::RuntimeResult;
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;

struct LogRuntime {
    calls: AtomicUsize,
    responses: Mutex<VecDeque<RuntimeResult<Vec<RuntimeLogChunk>>>>,
}

impl LogRuntime {
    fn new(responses: Vec<RuntimeResult<Vec<RuntimeLogChunk>>>) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            responses: Mutex::new(responses.into()),
        }
    }
}

#[async_trait]
impl RuntimeClient for LogRuntime {
    async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        Err(RuntimeError::Protocol("unexpected capabilities".into()))
    }

    async fn apply(&self, _request: &RuntimeApplyRequest) -> RuntimeResult<RuntimeObservation> {
        Err(RuntimeError::Protocol("unexpected apply".into()))
    }

    async fn inspect(&self, _unit_id: &str) -> RuntimeResult<RuntimeInspection> {
        Err(RuntimeError::Protocol("unexpected inspect".into()))
    }

    async fn stop(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeInspection> {
        Err(RuntimeError::Protocol("unexpected stop".into()))
    }

    async fn remove(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeRemoval> {
        Err(RuntimeError::Protocol("unexpected remove".into()))
    }

    async fn logs(&self, query: &RuntimeLogQuery) -> RuntimeResult<Vec<RuntimeLogChunk>> {
        query.validate().map_err(RuntimeError::Protocol)?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.responses
            .lock()
            .await
            .pop_front()
            .unwrap_or_else(|| Ok(Vec::new()))
    }

    async fn exec(&self, _request: &RuntimeExecRequest) -> RuntimeResult<RuntimeExecResult> {
        Err(RuntimeError::Protocol("unexpected exec".into()))
    }
}

struct LogTransport {
    failures: AtomicUsize,
    accepted_override: Mutex<Option<u16>>,
    batches: Mutex<Vec<NodeLogChunkBatch>>,
}

impl LogTransport {
    fn new(failures: usize) -> Self {
        Self {
            failures: AtomicUsize::new(failures),
            accepted_override: Mutex::new(None),
            batches: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl NodeControlTransport for LogTransport {
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
        _batch: &NodeObservationBatch,
    ) -> Result<NodeObservationReceipt, NodeControlClientError> {
        Err(NodeControlClientError::Invalid(
            "unexpected observations".into(),
        ))
    }

    async fn record_log_chunks(
        &self,
        batch: &NodeLogChunkBatch,
    ) -> Result<NodeLogChunkReceipt, NodeControlClientError> {
        batch.validate().map_err(NodeControlClientError::Invalid)?;
        self.batches.lock().await.push(batch.clone());
        if self.failures.load(Ordering::SeqCst) > 0 {
            self.failures.fetch_sub(1, Ordering::SeqCst);
            return Err(NodeControlClientError::Transport(
                "injected upload interruption".into(),
            ));
        }
        Ok(NodeLogChunkReceipt {
            schema: NodeLogChunkReceipt::SCHEMA.into(),
            batch_id: batch.batch_id,
            node_id: batch.node_id,
            accepted_chunks: self
                .accepted_override
                .lock()
                .await
                .unwrap_or_else(|| u16::try_from(batch.chunks.len()).expect("bounded batch")),
            replayed: self.batches.lock().await.len() > 1,
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

fn config(max_batch_chunks: u16) -> LogShippingConfig {
    LogShippingConfig {
        poll_interval_ms: 10,
        max_batch_chunks,
        max_batch_bytes: 1024 * 1024,
    }
}

fn target() -> RuntimeLogTarget {
    RuntimeLogTarget {
        unit_id: "service-1".into(),
        generation: 1,
    }
}

fn chunk(sequence: u64) -> RuntimeLogChunk {
    RuntimeLogChunk {
        schema: RuntimeLogChunk::SCHEMA.into(),
        cursor: format!("opaque:{sequence}"),
        sequence,
        observed_at_ms: sequence,
        stream: RuntimeLogStream::Stdout,
        data: format!("line {sequence}\n"),
    }
}

#[tokio::test]
async fn restart_replays_the_exact_bounded_batch_before_reading_more_logs() {
    let directory = tempfile::tempdir().expect("state directory");
    let node_id = Uuid::now_v7();
    let targets = vec![target()];
    let first_runtime = Arc::new(LogRuntime::new(vec![Ok(vec![chunk(1), chunk(2)])]));
    let first_transport = Arc::new(LogTransport::new(1));
    let first = LogShipper::new(
        node_id,
        first_runtime,
        first_transport.clone(),
        directory.path().to_owned(),
        config(2),
    )
    .expect("first shipper");

    let error = first
        .ship_once(&targets)
        .await
        .expect_err("first upload interruption");
    assert!(error.retryable());
    let persisted = first
        .state
        .snapshot(targets.clone())
        .await
        .expect("pending state");
    let pending = persisted.pending.as_ref().expect("durable pending batch");
    assert_eq!(pending.chunks.len(), 2);
    assert!(persisted.cursor("service-1", 1).is_none());
    let first_batch = first_transport
        .batches
        .lock()
        .await
        .first()
        .cloned()
        .expect("first upload");
    assert_eq!(pending, &first_batch);

    let restarted_runtime = Arc::new(LogRuntime::new(Vec::new()));
    let restarted_transport = Arc::new(LogTransport::new(0));
    let restarted = LogShipper::new(
        node_id,
        restarted_runtime.clone(),
        restarted_transport.clone(),
        directory.path().to_owned(),
        config(2),
    )
    .expect("restarted shipper");
    assert!(restarted
        .ship_once(&targets)
        .await
        .expect("replay pending batch"));
    assert_eq!(restarted_runtime.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        restarted_transport
            .batches
            .lock()
            .await
            .first()
            .cloned()
            .expect("replayed upload"),
        first_batch
    );

    let committed = restarted
        .state
        .snapshot(targets.clone())
        .await
        .expect("committed state");
    assert!(committed.pending.is_none());
    assert_eq!(
        committed
            .cursor("service-1", 1)
            .expect("committed cursor")
            .sequence,
        2
    );
    assert!(!restarted
        .ship_once(&targets)
        .await
        .expect("empty follow-up read"));
    assert_eq!(restarted_runtime.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn invalid_receipt_keeps_the_pending_batch_and_does_not_advance_the_cursor() {
    let directory = tempfile::tempdir().expect("state directory");
    let node_id = Uuid::now_v7();
    let targets = vec![target()];
    let runtime = Arc::new(LogRuntime::new(vec![Ok(vec![chunk(1)])]));
    let transport = Arc::new(LogTransport::new(0));
    *transport.accepted_override.lock().await = Some(2);
    let shipper = LogShipper::new(
        node_id,
        runtime.clone(),
        transport.clone(),
        directory.path().to_owned(),
        config(2),
    )
    .expect("shipper");

    assert!(matches!(
        shipper.ship_once(&targets).await,
        Err(LogShippingError::Invalid(_))
    ));
    let pending = shipper
        .state
        .snapshot(targets.clone())
        .await
        .expect("pending state");
    assert!(pending.pending.is_some());
    assert!(pending.cursor("service-1", 1).is_none());

    *transport.accepted_override.lock().await = None;
    assert!(shipper.ship_once(&targets).await.expect("receipt retry"));
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
    let committed = shipper
        .state
        .snapshot(targets)
        .await
        .expect("committed state");
    assert!(committed.pending.is_none());
    assert_eq!(
        committed
            .cursor("service-1", 1)
            .expect("committed cursor")
            .sequence,
        1
    );
}
