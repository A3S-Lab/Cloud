use crate::code_harness::{resolve_runtime_endpoint, CodeHarnessError, SharedCodeHarnessTransport};
use crate::outbound_batch::{DurableOutboundBatch, OutboundBatchError, OutboundBatchProtocol};
use crate::state_file::{self, StateLock};
use crate::{NodeControlClientError, NodeControlTransport};
use a3s_cloud_contracts::{
    AgentProtocolEventPageRequestV1, AgentProtocolRunStateV1, NodeCodeAgentEventBatchV1,
    NodeCodeAgentEventReceiptV1, NodeCodeAgentRuntimeBindingV1,
};
use a3s_runtime::RuntimeClient;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const CODE_EVENT_SHIPPING_FILE: &str = "code-agent-event-shipping.json";
const CODE_EVENT_SHIPPING_LOCK_FILE: &str = "code-agent-event-shipping.lock";
const CODE_EVENT_PAGE_LIMIT: u16 = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DurableCodeEventCursor {
    binding: NodeCodeAgentRuntimeBindingV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    after_event_sequence: Option<u64>,
    observed_state: AgentProtocolRunStateV1,
    observed_at_ms: u64,
    drained: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CodeEventShippingState {
    schema: String,
    node_id: Uuid,
    cursors: BTreeMap<String, DurableCodeEventCursor>,
    pending: DurableOutboundBatch<NodeCodeAgentEventBatchV1>,
}

impl OutboundBatchProtocol for NodeCodeAgentEventBatchV1 {
    type Receipt = NodeCodeAgentEventReceiptV1;

    fn validate(&self) -> Result<(), String> {
        NodeCodeAgentEventBatchV1::validate(self)
    }

    fn validate_receipt(&self, receipt: &Self::Receipt) -> Result<(), String> {
        receipt.validate_for(self)
    }
}

impl CodeEventShippingState {
    const SCHEMA: &'static str = "a3s.cloud.node-code-agent-event-shipping-state.v1";

    fn empty(node_id: Uuid) -> Self {
        Self {
            schema: Self::SCHEMA.into(),
            node_id,
            cursors: BTreeMap::new(),
            pending: DurableOutboundBatch::empty(),
        }
    }

    fn validate(&self, expected_node_id: Uuid) -> Result<(), CodeEventShippingError> {
        if self.schema != Self::SCHEMA || self.node_id.is_nil() || self.node_id != expected_node_id
        {
            return Err(CodeEventShippingError::Invalid(
                "Code event shipping state schema or node identity is invalid".into(),
            ));
        }
        for (identity_digest, cursor) in &self.cursors {
            cursor
                .binding
                .validate()
                .map_err(CodeEventShippingError::Invalid)?;
            if binding_key(&cursor.binding)? != *identity_digest
                || cursor.observed_at_ms == 0
                || cursor.drained && !cursor.observed_state.is_terminal()
            {
                return Err(CodeEventShippingError::Invalid(
                    "durable Code event cursor is invalid".into(),
                ));
            }
        }
        self.pending.validate().map_err(outbound_error)?;
        if let Some(pending) = self.pending.pending() {
            if pending.node_id != self.node_id {
                return Err(CodeEventShippingError::Invalid(
                    "pending Code event batch belongs to another node".into(),
                ));
            }
            let identity_digest = binding_key(&pending.binding)?;
            match self.cursors.get(&identity_digest) {
                Some(cursor)
                    if cursor.binding == pending.binding
                        && !cursor.drained
                        && cursor.after_event_sequence == pending.page.after_event_sequence
                        && cursor.observed_at_ms <= pending.page.observed_at_ms => {}
                None if pending.page.after_event_sequence.is_none() => {}
                _ => {
                    return Err(CodeEventShippingError::Invalid(
                        "pending Code event page does not continue its durable cursor".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn cursor(
        &self,
        binding: &NodeCodeAgentRuntimeBindingV1,
    ) -> Result<Option<&DurableCodeEventCursor>, CodeEventShippingError> {
        let key = binding_key(binding)?;
        let cursor = self.cursors.get(&key);
        if cursor.is_some_and(|cursor| &cursor.binding != binding) {
            return Err(CodeEventShippingError::Conflict(
                "one A3S Code run identity changed its Runtime binding".into(),
            ));
        }
        Ok(cursor)
    }

    fn retain_bindings(
        &mut self,
        bindings: &[NodeCodeAgentRuntimeBindingV1],
    ) -> Result<(), CodeEventShippingError> {
        let mut active = BTreeMap::new();
        let mut executions = BTreeSet::new();
        for binding in bindings {
            binding
                .validate()
                .map_err(CodeEventShippingError::Invalid)?;
            let key = binding_key(binding)?;
            if !executions.insert(binding.execution_id) || active.insert(key, binding).is_some() {
                return Err(CodeEventShippingError::Invalid(
                    "Code event target list contains duplicate execution or run identities".into(),
                ));
            }
        }
        for (key, binding) in &active {
            if self
                .cursors
                .get(key)
                .is_some_and(|cursor| &cursor.binding != *binding)
            {
                return Err(CodeEventShippingError::Conflict(
                    "one A3S Code run identity changed its Runtime binding".into(),
                ));
            }
        }
        self.cursors.retain(|key, _| active.contains_key(key));
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct FileCodeEventShippingState {
    root: PathBuf,
    node_id: Uuid,
}

impl FileCodeEventShippingState {
    fn new(root: impl Into<PathBuf>, node_id: Uuid) -> Result<Self, CodeEventShippingError> {
        if node_id.is_nil() {
            return Err(CodeEventShippingError::Invalid(
                "Code event shipping node ID must not be nil".into(),
            ));
        }
        Ok(Self {
            root: root.into(),
            node_id,
        })
    }

    async fn snapshot(
        &self,
        bindings: Vec<NodeCodeAgentRuntimeBindingV1>,
    ) -> Result<CodeEventShippingState, CodeEventShippingError> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || state.snapshot_sync(&bindings))
            .await
            .map_err(task_error)?
    }

    async fn set_pending(
        &self,
        batch: NodeCodeAgentEventBatchV1,
    ) -> Result<(), CodeEventShippingError> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || state.set_pending_sync(batch))
            .await
            .map_err(task_error)?
    }

    async fn commit(
        &self,
        receipt: NodeCodeAgentEventReceiptV1,
    ) -> Result<(), CodeEventShippingError> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || state.commit_sync(receipt))
            .await
            .map_err(task_error)?
    }

    fn snapshot_sync(
        &self,
        bindings: &[NodeCodeAgentRuntimeBindingV1],
    ) -> Result<CodeEventShippingState, CodeEventShippingError> {
        state_file::ensure_directory(&self.root).map_err(state_error)?;
        let _lock = StateLock::exclusive(&self.root.join(CODE_EVENT_SHIPPING_LOCK_FILE))
            .map_err(state_error)?;
        let mut state = self.read_state()?;
        if !state.pending.is_pending() {
            let original = state.cursors.clone();
            state.retain_bindings(bindings)?;
            if state.cursors != original {
                state.validate(self.node_id)?;
                self.write_state(&state)?;
            }
        }
        Ok(state)
    }

    fn set_pending_sync(
        &self,
        batch: NodeCodeAgentEventBatchV1,
    ) -> Result<(), CodeEventShippingError> {
        batch.validate().map_err(CodeEventShippingError::Invalid)?;
        if batch.node_id != self.node_id {
            return Err(CodeEventShippingError::Invalid(
                "pending Code event batch belongs to another node".into(),
            ));
        }
        state_file::ensure_directory(&self.root).map_err(state_error)?;
        let _lock = StateLock::exclusive(&self.root.join(CODE_EVENT_SHIPPING_LOCK_FILE))
            .map_err(state_error)?;
        let mut state = self.read_state()?;
        state.pending.stage(batch).map_err(outbound_error)?;
        state.validate(self.node_id)?;
        self.write_state(&state)
    }

    fn commit_sync(
        &self,
        receipt: NodeCodeAgentEventReceiptV1,
    ) -> Result<(), CodeEventShippingError> {
        state_file::ensure_directory(&self.root).map_err(state_error)?;
        let _lock = StateLock::exclusive(&self.root.join(CODE_EVENT_SHIPPING_LOCK_FILE))
            .map_err(state_error)?;
        let mut state = self.read_state()?;
        let pending = state
            .pending
            .acknowledge(&receipt)
            .map_err(outbound_error)?;
        let identity_digest = binding_key(&pending.binding)?;
        if state
            .cursors
            .get(&identity_digest)
            .is_some_and(|cursor| cursor.binding != pending.binding)
        {
            return Err(CodeEventShippingError::Conflict(
                "one A3S Code run identity changed its Runtime binding".into(),
            ));
        }
        state.cursors.insert(
            identity_digest,
            DurableCodeEventCursor {
                binding: pending.binding,
                after_event_sequence: pending.page.next_after_event_sequence,
                observed_state: pending.page.state,
                observed_at_ms: pending.page.observed_at_ms,
                drained: pending.page.state.is_terminal() && !pending.page.has_more,
            },
        );
        state.validate(self.node_id)?;
        self.write_state(&state)
    }

    fn read_state(&self) -> Result<CodeEventShippingState, CodeEventShippingError> {
        let path = self.root.join(CODE_EVENT_SHIPPING_FILE);
        let state = state_file::read_json(&path, "node Code event shipping state")
            .map_err(state_error)?
            .unwrap_or_else(|| CodeEventShippingState::empty(self.node_id));
        state.validate(self.node_id)?;
        Ok(state)
    }

    fn write_state(&self, state: &CodeEventShippingState) -> Result<(), CodeEventShippingError> {
        state_file::atomic_write(&self.root.join(CODE_EVENT_SHIPPING_FILE), state)
            .map_err(state_error)
    }
}

/// Reliable transport projection from the Code-owned event endpoint to Cloud.
/// It owns only an outbound delivery cursor; A3S Code remains the run and event
/// lifecycle authority.
pub(crate) struct CodeEventShipper {
    node_id: Uuid,
    runtime: Arc<dyn RuntimeClient>,
    harness: SharedCodeHarnessTransport,
    transport: Arc<dyn NodeControlTransport>,
    state: FileCodeEventShippingState,
    request_timeout: Duration,
}

impl CodeEventShipper {
    pub(crate) fn new(
        node_id: Uuid,
        runtime: Arc<dyn RuntimeClient>,
        harness: SharedCodeHarnessTransport,
        transport: Arc<dyn NodeControlTransport>,
        state_dir: PathBuf,
        request_timeout: Duration,
    ) -> Result<Self, CodeEventShippingError> {
        if request_timeout.is_zero() {
            return Err(CodeEventShippingError::Invalid(
                "Code event request timeout must be positive".into(),
            ));
        }
        Ok(Self {
            node_id,
            runtime,
            harness,
            transport,
            state: FileCodeEventShippingState::new(state_dir, node_id)?,
            request_timeout,
        })
    }

    pub(crate) async fn ship_once(
        &self,
        bindings: &[NodeCodeAgentRuntimeBindingV1],
    ) -> Result<bool, CodeEventShippingError> {
        validate_bindings(bindings)?;
        let snapshot = self.state.snapshot(bindings.to_vec()).await?;
        if let Some(pending) = snapshot.pending.pending() {
            self.upload(pending.clone()).await?;
            return Ok(true);
        }
        let Some(batch) = self.collect(bindings, &snapshot).await? else {
            return Ok(false);
        };
        self.state.set_pending(batch.clone()).await?;
        self.upload(batch).await?;
        Ok(true)
    }

    async fn upload(&self, batch: NodeCodeAgentEventBatchV1) -> Result<(), CodeEventShippingError> {
        let receipt = self.transport.record_code_agent_events(&batch).await?;
        self.state.commit(receipt).await
    }

    async fn collect(
        &self,
        bindings: &[NodeCodeAgentRuntimeBindingV1],
        snapshot: &CodeEventShippingState,
    ) -> Result<Option<NodeCodeAgentEventBatchV1>, CodeEventShippingError> {
        let mut candidates = bindings
            .iter()
            .map(|binding| {
                let cursor = snapshot.cursor(binding)?;
                Ok((
                    cursor.map_or(0, |cursor| cursor.observed_at_ms),
                    binding.execution_id,
                    binding,
                    cursor,
                ))
            })
            .collect::<Result<Vec<_>, CodeEventShippingError>>()?;
        candidates
            .sort_by_key(|(observed_at_ms, execution_id, _, _)| (*observed_at_ms, *execution_id));

        for (_, _, binding, cursor) in candidates {
            if cursor.is_some_and(|cursor| cursor.drained) {
                continue;
            }
            let endpoint = match resolve_runtime_endpoint(self.runtime.as_ref(), binding).await {
                Ok(endpoint) => endpoint,
                Err(error) if error.is_unavailable() => continue,
                Err(error) => return Err(error.into()),
            };
            let request = AgentProtocolEventPageRequestV1 {
                schema: AgentProtocolEventPageRequestV1::SCHEMA.into(),
                identity: binding.code_run_identity.clone(),
                after_event_sequence: cursor.and_then(|cursor| cursor.after_event_sequence),
                limit: CODE_EVENT_PAGE_LIMIT,
            };
            request
                .validate()
                .map_err(|error| CodeEventShippingError::Invalid(error.code().into()))?;
            let page = self
                .harness
                .event_page(&endpoint, &request, self.request_timeout)
                .await?;
            if page.retention_gap {
                return Err(CodeEventShippingError::Invalid(
                    "A3S Code event retention gap requires execution recovery".into(),
                ));
            }
            let changed = cursor.is_none_or(|cursor| {
                cursor.after_event_sequence != page.next_after_event_sequence
                    || cursor.observed_state != page.state
                    || page.state.is_terminal() && !cursor.drained
            });
            if page.events.is_empty() && !changed {
                continue;
            }
            let batch = NodeCodeAgentEventBatchV1 {
                schema: NodeCodeAgentEventBatchV1::SCHEMA.into(),
                batch_id: Uuid::now_v7(),
                node_id: self.node_id,
                binding: binding.clone(),
                sent_at_ms: current_time_ms()?.max(page.observed_at_ms),
                page,
            };
            batch.validate().map_err(CodeEventShippingError::Invalid)?;
            return Ok(Some(batch));
        }
        Ok(None)
    }
}

fn validate_bindings(
    bindings: &[NodeCodeAgentRuntimeBindingV1],
) -> Result<(), CodeEventShippingError> {
    let mut executions = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for binding in bindings {
        binding
            .validate()
            .map_err(CodeEventShippingError::Invalid)?;
        if !executions.insert(binding.execution_id) || !identities.insert(binding_key(binding)?) {
            return Err(CodeEventShippingError::Invalid(
                "Code event target list contains duplicate execution or run identities".into(),
            ));
        }
    }
    Ok(())
}

fn binding_key(binding: &NodeCodeAgentRuntimeBindingV1) -> Result<String, CodeEventShippingError> {
    binding
        .code_run_identity
        .digest()
        .map_err(|error| CodeEventShippingError::Invalid(error.code().into()))
}

fn current_time_ms() -> Result<u64, CodeEventShippingError> {
    u64::try_from(Utc::now().timestamp_millis())
        .map_err(|_| CodeEventShippingError::Invalid("current timestamp is invalid".into()))
}

fn state_error(error: state_file::SecureStateError) -> CodeEventShippingError {
    CodeEventShippingError::State(error.to_string())
}

fn outbound_error(error: OutboundBatchError) -> CodeEventShippingError {
    match error {
        OutboundBatchError::Invalid(message) => CodeEventShippingError::Invalid(message),
        OutboundBatchError::Conflict(message) => CodeEventShippingError::Conflict(message),
    }
}

fn task_error(error: tokio::task::JoinError) -> CodeEventShippingError {
    CodeEventShippingError::State(format!("Code event shipping state task failed: {error}"))
}

#[derive(Debug, thiserror::Error)]
pub enum CodeEventShippingError {
    #[error("invalid Code event shipping data: {0}")]
    Invalid(String),
    #[error("Code event shipping state conflict: {0}")]
    Conflict(String),
    #[error("Code event shipping state failed: {0}")]
    State(String),
    #[error(transparent)]
    ControlPlane(#[from] NodeControlClientError),
    #[error("A3S Code Harness event shipping failed: {message}")]
    Harness { message: String, retryable: bool },
}

impl CodeEventShippingError {
    pub fn retryable(&self) -> bool {
        match self {
            Self::ControlPlane(error) => error.retryable(),
            Self::Harness { retryable, .. } => *retryable,
            Self::Invalid(_) | Self::Conflict(_) | Self::State(_) => false,
        }
    }
}

impl From<CodeHarnessError> for CodeEventShippingError {
    fn from(error: CodeHarnessError) -> Self {
        Self::Harness {
            retryable: error.retryable(),
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
#[path = "code_event_shipper_tests.rs"]
mod tests;
