use crate::agent_provider_harness::{
    resolve_runtime_endpoint, validate_reference_echo_binding, AgentProviderHarnessError,
    SharedAgentProviderHarnessTransport,
};
use crate::outbound_batch::{DurableOutboundBatch, OutboundBatchError, OutboundBatchProtocol};
use crate::state_file::{self, StateLock};
use crate::{NodeControlClientError, NodeControlTransport};
use a3s_cloud_contracts::{
    AgentProviderEventPageRequestV1, AgentProviderRunStateV1, NodeAgentProviderEventBatchV1,
    NodeAgentProviderEventReceiptV1, NodeAgentProviderRuntimeBindingV1,
};
use a3s_runtime::RuntimeClient;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const PROVIDER_EVENT_SHIPPING_FILE: &str = "agent-provider-event-shipping.json";
const PROVIDER_EVENT_SHIPPING_LOCK_FILE: &str = "agent-provider-event-shipping.lock";
const PROVIDER_EVENT_PAGE_LIMIT: u16 = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DurableProviderEventCursor {
    binding: NodeAgentProviderRuntimeBindingV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    after_event_sequence: Option<u64>,
    observed_state: AgentProviderRunStateV1,
    observed_at_ms: u64,
    drained: bool,
    #[serde(default)]
    recovery_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderEventShippingState {
    schema: String,
    node_id: Uuid,
    cursors: BTreeMap<String, DurableProviderEventCursor>,
    pending: DurableOutboundBatch<NodeAgentProviderEventBatchV1>,
}

impl OutboundBatchProtocol for NodeAgentProviderEventBatchV1 {
    type Receipt = NodeAgentProviderEventReceiptV1;

    fn validate(&self) -> Result<(), String> {
        NodeAgentProviderEventBatchV1::validate(self)
    }

    fn validate_receipt(&self, receipt: &Self::Receipt) -> Result<(), String> {
        receipt.validate_for(self)
    }
}

impl ProviderEventShippingState {
    const SCHEMA: &'static str = "a3s.cloud.node-agent-provider-event-shipping-state.v1";

    fn empty(node_id: Uuid) -> Self {
        Self {
            schema: Self::SCHEMA.into(),
            node_id,
            cursors: BTreeMap::new(),
            pending: DurableOutboundBatch::empty(),
        }
    }

    fn validate(&self, expected_node_id: Uuid) -> Result<(), AgentProviderEventShippingError> {
        if self.schema != Self::SCHEMA || self.node_id.is_nil() || self.node_id != expected_node_id
        {
            return Err(AgentProviderEventShippingError::Invalid(
                "Agent provider event shipping state schema or node identity is invalid".into(),
            ));
        }
        for (identity_digest, cursor) in &self.cursors {
            validate_reference_echo_binding(&cursor.binding).map_err(harness_error)?;
            if binding_key(&cursor.binding)? != *identity_digest
                || cursor.observed_at_ms == 0
                || cursor.drained
                    && !cursor.observed_state.is_terminal()
                    && !cursor.recovery_required
                || cursor.recovery_required && !cursor.drained
            {
                return Err(AgentProviderEventShippingError::Invalid(
                    "durable Agent provider event cursor is invalid".into(),
                ));
            }
        }
        self.pending.validate().map_err(outbound_error)?;
        if let Some(pending) = self.pending.pending() {
            if pending.node_id != self.node_id {
                return Err(AgentProviderEventShippingError::Invalid(
                    "pending Agent provider event batch belongs to another node".into(),
                ));
            }
            validate_reference_echo_binding(&pending.binding).map_err(harness_error)?;
            let identity_digest = binding_key(&pending.binding)?;
            match self.cursors.get(&identity_digest) {
                Some(cursor)
                    if cursor.binding == pending.binding
                        && !cursor.drained
                        && cursor.after_event_sequence == pending.page.after_event_sequence
                        && cursor.observed_at_ms <= pending.page.observed_at_ms => {}
                None if pending.page.after_event_sequence.is_none() => {}
                _ => {
                    return Err(AgentProviderEventShippingError::Invalid(
                        "pending Agent provider event page does not continue its durable cursor"
                            .into(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn cursor(
        &self,
        binding: &NodeAgentProviderRuntimeBindingV1,
    ) -> Result<Option<&DurableProviderEventCursor>, AgentProviderEventShippingError> {
        let key = binding_key(binding)?;
        let cursor = self.cursors.get(&key);
        if cursor.is_some_and(|cursor| &cursor.binding != binding) {
            return Err(AgentProviderEventShippingError::Conflict(
                "one Agent provider run identity changed its Runtime binding".into(),
            ));
        }
        Ok(cursor)
    }

    fn retain_bindings(
        &mut self,
        bindings: &[NodeAgentProviderRuntimeBindingV1],
    ) -> Result<(), AgentProviderEventShippingError> {
        let mut active = BTreeMap::new();
        let mut executions = BTreeSet::new();
        for binding in bindings {
            validate_reference_echo_binding(binding).map_err(harness_error)?;
            let key = binding_key(binding)?;
            if !executions.insert(binding.execution_id) || active.insert(key, binding).is_some() {
                return Err(AgentProviderEventShippingError::Invalid(
                    "Agent provider event targets contain duplicate execution or run identities"
                        .into(),
                ));
            }
        }
        for (key, binding) in &active {
            if self
                .cursors
                .get(key)
                .is_some_and(|cursor| &cursor.binding != *binding)
            {
                return Err(AgentProviderEventShippingError::Conflict(
                    "one Agent provider run identity changed its Runtime binding".into(),
                ));
            }
        }
        self.cursors.retain(|key, _| active.contains_key(key));
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct FileProviderEventShippingState {
    root: PathBuf,
    node_id: Uuid,
}

impl FileProviderEventShippingState {
    fn new(
        root: impl Into<PathBuf>,
        node_id: Uuid,
    ) -> Result<Self, AgentProviderEventShippingError> {
        if node_id.is_nil() {
            return Err(AgentProviderEventShippingError::Invalid(
                "Agent provider event shipping node ID must not be nil".into(),
            ));
        }
        Ok(Self {
            root: root.into(),
            node_id,
        })
    }

    async fn snapshot(
        &self,
        bindings: Vec<NodeAgentProviderRuntimeBindingV1>,
    ) -> Result<ProviderEventShippingState, AgentProviderEventShippingError> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || state.snapshot_sync(&bindings))
            .await
            .map_err(task_error)?
    }

    async fn set_pending(
        &self,
        batch: NodeAgentProviderEventBatchV1,
    ) -> Result<(), AgentProviderEventShippingError> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || state.set_pending_sync(batch))
            .await
            .map_err(task_error)?
    }

    async fn commit(
        &self,
        receipt: NodeAgentProviderEventReceiptV1,
    ) -> Result<(), AgentProviderEventShippingError> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || state.commit_sync(receipt))
            .await
            .map_err(task_error)?
    }

    fn snapshot_sync(
        &self,
        bindings: &[NodeAgentProviderRuntimeBindingV1],
    ) -> Result<ProviderEventShippingState, AgentProviderEventShippingError> {
        state_file::ensure_directory(&self.root).map_err(state_error)?;
        let _lock = StateLock::exclusive(&self.root.join(PROVIDER_EVENT_SHIPPING_LOCK_FILE))
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
        batch: NodeAgentProviderEventBatchV1,
    ) -> Result<(), AgentProviderEventShippingError> {
        batch
            .validate()
            .map_err(AgentProviderEventShippingError::Invalid)?;
        validate_reference_echo_binding(&batch.binding).map_err(harness_error)?;
        if batch.node_id != self.node_id {
            return Err(AgentProviderEventShippingError::Invalid(
                "pending Agent provider event batch belongs to another node".into(),
            ));
        }
        state_file::ensure_directory(&self.root).map_err(state_error)?;
        let _lock = StateLock::exclusive(&self.root.join(PROVIDER_EVENT_SHIPPING_LOCK_FILE))
            .map_err(state_error)?;
        let mut state = self.read_state()?;
        state.pending.stage(batch).map_err(outbound_error)?;
        state.validate(self.node_id)?;
        self.write_state(&state)
    }

    fn commit_sync(
        &self,
        receipt: NodeAgentProviderEventReceiptV1,
    ) -> Result<(), AgentProviderEventShippingError> {
        state_file::ensure_directory(&self.root).map_err(state_error)?;
        let _lock = StateLock::exclusive(&self.root.join(PROVIDER_EVENT_SHIPPING_LOCK_FILE))
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
            return Err(AgentProviderEventShippingError::Conflict(
                "one Agent provider run identity changed its Runtime binding".into(),
            ));
        }
        state.cursors.insert(
            identity_digest,
            DurableProviderEventCursor {
                binding: pending.binding,
                after_event_sequence: pending.page.next_after_event_sequence,
                observed_state: pending.page.state,
                observed_at_ms: pending.page.observed_at_ms,
                drained: pending.page.retention_gap
                    || pending.page.state.is_terminal() && !pending.page.has_more,
                recovery_required: pending.page.retention_gap,
            },
        );
        state.validate(self.node_id)?;
        self.write_state(&state)
    }

    fn read_state(&self) -> Result<ProviderEventShippingState, AgentProviderEventShippingError> {
        let state = state_file::read_json(
            &self.root.join(PROVIDER_EVENT_SHIPPING_FILE),
            "node Agent provider event shipping state",
        )
        .map_err(state_error)?
        .unwrap_or_else(|| ProviderEventShippingState::empty(self.node_id));
        state.validate(self.node_id)?;
        Ok(state)
    }

    fn write_state(
        &self,
        state: &ProviderEventShippingState,
    ) -> Result<(), AgentProviderEventShippingError> {
        state_file::atomic_write(&self.root.join(PROVIDER_EVENT_SHIPPING_FILE), state)
            .map_err(state_error)
    }
}

/// Reliable projection from a common provider event page into the existing
/// Fleet delivery channel and Agents-owned semantic sequence.
pub(crate) struct AgentProviderEventShipper {
    node_id: Uuid,
    runtime: Arc<dyn RuntimeClient>,
    harness: SharedAgentProviderHarnessTransport,
    transport: Arc<dyn NodeControlTransport>,
    state: FileProviderEventShippingState,
    request_timeout: Duration,
}

impl AgentProviderEventShipper {
    pub(crate) fn new(
        node_id: Uuid,
        runtime: Arc<dyn RuntimeClient>,
        harness: SharedAgentProviderHarnessTransport,
        transport: Arc<dyn NodeControlTransport>,
        state_dir: PathBuf,
        request_timeout: Duration,
    ) -> Result<Self, AgentProviderEventShippingError> {
        if request_timeout.is_zero() {
            return Err(AgentProviderEventShippingError::Invalid(
                "Agent provider event request timeout must be positive".into(),
            ));
        }
        Ok(Self {
            node_id,
            runtime,
            harness,
            transport,
            state: FileProviderEventShippingState::new(state_dir, node_id)?,
            request_timeout,
        })
    }

    pub(crate) async fn ship_once(
        &self,
        bindings: &[NodeAgentProviderRuntimeBindingV1],
    ) -> Result<bool, AgentProviderEventShippingError> {
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

    async fn upload(
        &self,
        batch: NodeAgentProviderEventBatchV1,
    ) -> Result<(), AgentProviderEventShippingError> {
        let receipt = self.transport.record_agent_provider_events(&batch).await?;
        self.state.commit(receipt).await
    }

    async fn collect(
        &self,
        bindings: &[NodeAgentProviderRuntimeBindingV1],
        snapshot: &ProviderEventShippingState,
    ) -> Result<Option<NodeAgentProviderEventBatchV1>, AgentProviderEventShippingError> {
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
            .collect::<Result<Vec<_>, AgentProviderEventShippingError>>()?;
        candidates
            .sort_by_key(|(observed_at_ms, execution_id, _, _)| (*observed_at_ms, *execution_id));

        for (_, _, binding, cursor) in candidates {
            if cursor.is_some_and(|cursor| cursor.drained) {
                continue;
            }
            let endpoint = match resolve_runtime_endpoint(self.runtime.as_ref(), binding).await {
                Ok(endpoint) => endpoint,
                Err(error) if error.is_unavailable() => continue,
                Err(error) => return Err(harness_error(error)),
            };
            let profile = binding
                .profile()
                .map_err(AgentProviderEventShippingError::Invalid)?;
            let request = AgentProviderEventPageRequestV1 {
                schema: AgentProviderEventPageRequestV1::SCHEMA.into(),
                identity: binding.provider_run_identity.clone(),
                after_event_sequence: cursor.and_then(|cursor| cursor.after_event_sequence),
                limit: PROVIDER_EVENT_PAGE_LIMIT,
            };
            request
                .validate_for(&profile)
                .map_err(AgentProviderEventShippingError::Invalid)?;
            let page = self
                .harness
                .event_page(&endpoint, binding, &request, self.request_timeout)
                .await
                .map_err(harness_error)?;
            let changed = page.retention_gap
                || cursor.is_none_or(|cursor| {
                    cursor.after_event_sequence != page.next_after_event_sequence
                        || cursor.observed_state != page.state
                        || page.state.is_terminal() && !cursor.drained
                });
            if page.events.is_empty() && !changed {
                continue;
            }
            let batch = NodeAgentProviderEventBatchV1 {
                schema: NodeAgentProviderEventBatchV1::SCHEMA.into(),
                batch_id: Uuid::now_v7(),
                node_id: self.node_id,
                binding: binding.clone(),
                sent_at_ms: current_time_ms()?.max(page.observed_at_ms),
                page,
            };
            batch
                .validate()
                .map_err(AgentProviderEventShippingError::Invalid)?;
            return Ok(Some(batch));
        }
        Ok(None)
    }
}

fn validate_bindings(
    bindings: &[NodeAgentProviderRuntimeBindingV1],
) -> Result<(), AgentProviderEventShippingError> {
    let mut executions = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for binding in bindings {
        validate_reference_echo_binding(binding).map_err(harness_error)?;
        if !executions.insert(binding.execution_id) || !identities.insert(binding_key(binding)?) {
            return Err(AgentProviderEventShippingError::Invalid(
                "Agent provider event targets contain duplicate execution or run identities".into(),
            ));
        }
    }
    Ok(())
}

fn binding_key(
    binding: &NodeAgentProviderRuntimeBindingV1,
) -> Result<String, AgentProviderEventShippingError> {
    binding
        .provider_run_identity
        .digest()
        .map_err(AgentProviderEventShippingError::Invalid)
}

fn current_time_ms() -> Result<u64, AgentProviderEventShippingError> {
    u64::try_from(Utc::now().timestamp_millis()).map_err(|_| {
        AgentProviderEventShippingError::Invalid("current timestamp is invalid".into())
    })
}

fn state_error(error: state_file::SecureStateError) -> AgentProviderEventShippingError {
    AgentProviderEventShippingError::State(error.to_string())
}

fn outbound_error(error: OutboundBatchError) -> AgentProviderEventShippingError {
    match error {
        OutboundBatchError::Invalid(message) => AgentProviderEventShippingError::Invalid(message),
        OutboundBatchError::Conflict(message) => AgentProviderEventShippingError::Conflict(message),
    }
}

fn task_error(error: tokio::task::JoinError) -> AgentProviderEventShippingError {
    AgentProviderEventShippingError::State(format!(
        "Agent provider event shipping state task failed: {error}"
    ))
}

fn harness_error(error: AgentProviderHarnessError) -> AgentProviderEventShippingError {
    AgentProviderEventShippingError::Harness {
        retryable: error.retryable(),
        message: error.to_string(),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgentProviderEventShippingError {
    #[error("invalid Agent provider event shipping data: {0}")]
    Invalid(String),
    #[error("Agent provider event shipping state conflict: {0}")]
    Conflict(String),
    #[error("Agent provider event shipping state failed: {0}")]
    State(String),
    #[error(transparent)]
    ControlPlane(#[from] NodeControlClientError),
    #[error("Agent provider Harness event shipping failed: {message}")]
    Harness { message: String, retryable: bool },
}

impl AgentProviderEventShippingError {
    pub fn retryable(&self) -> bool {
        match self {
            Self::ControlPlane(error) => error.retryable(),
            Self::Harness { retryable, .. } => *retryable,
            Self::Invalid(_) | Self::Conflict(_) | Self::State(_) => false,
        }
    }
}

#[cfg(test)]
#[path = "agent_provider_event_shipper_tests.rs"]
mod tests;
