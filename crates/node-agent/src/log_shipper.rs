use crate::state_file::{self, StateLock};
use crate::{LogShippingConfig, NodeControlClientError, NodeControlTransport, RuntimeLogTarget};
use a3s_cloud_contracts::{NodeLogChunkBatch, NodeLogChunkReceipt, NodeLogChunkReport};
use a3s_runtime::contract::RuntimeLogQuery;
use a3s_runtime::{RuntimeClient, RuntimeError};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

const LOG_SHIPPING_FILE: &str = "log-shipping.json";
const LOG_SHIPPING_LOCK_FILE: &str = "log-shipping.lock";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DurableLogCursor {
    cursor: String,
    sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LogShippingState {
    schema: String,
    node_id: Uuid,
    cursors: BTreeMap<String, BTreeMap<u64, DurableLogCursor>>,
    pending: Option<NodeLogChunkBatch>,
}

impl LogShippingState {
    const SCHEMA: &'static str = "a3s.cloud.node-log-shipping-state.v1";

    fn empty(node_id: Uuid) -> Self {
        Self {
            schema: Self::SCHEMA.into(),
            node_id,
            cursors: BTreeMap::new(),
            pending: None,
        }
    }

    fn validate(&self, expected_node_id: Uuid) -> Result<(), LogShippingError> {
        if self.schema != Self::SCHEMA || self.node_id.is_nil() || self.node_id != expected_node_id
        {
            return Err(LogShippingError::Invalid(
                "log shipping state schema or node identity is invalid".into(),
            ));
        }
        for (unit_id, generations) in &self.cursors {
            validate_target(unit_id, 1)?;
            for (generation, cursor) in generations {
                if *generation == 0
                    || cursor.cursor.is_empty()
                    || cursor.cursor.len() > 1024
                    || cursor.cursor.contains('\0')
                {
                    return Err(LogShippingError::Invalid(
                        "durable log cursor is invalid".into(),
                    ));
                }
            }
        }
        if let Some(pending) = &self.pending {
            pending.validate().map_err(LogShippingError::Invalid)?;
            if pending.node_id != self.node_id {
                return Err(LogShippingError::Invalid(
                    "pending log batch belongs to another node".into(),
                ));
            }
            let mut last_sequences = BTreeMap::<(&str, u64), u64>::new();
            for report in &pending.chunks {
                let key = (report.unit_id.as_str(), report.generation);
                let committed = self
                    .cursor(report.unit_id.as_str(), report.generation)
                    .map(|cursor| cursor.sequence);
                let previous = last_sequences.get(&key).copied().or(committed);
                if previous.is_some_and(|sequence| report.chunk.sequence <= sequence) {
                    return Err(LogShippingError::Invalid(
                        "pending log batch does not advance its durable cursor".into(),
                    ));
                }
                last_sequences.insert(key, report.chunk.sequence);
            }
        }
        Ok(())
    }

    fn cursor(&self, unit_id: &str, generation: u64) -> Option<&DurableLogCursor> {
        self.cursors
            .get(unit_id)
            .and_then(|generations| generations.get(&generation))
    }

    fn retain_targets(&mut self, targets: &[RuntimeLogTarget]) {
        let active = targets
            .iter()
            .map(|target| (target.unit_id.as_str(), target.generation))
            .collect::<BTreeSet<_>>();
        self.cursors.retain(|unit_id, generations| {
            generations.retain(|generation, _| active.contains(&(unit_id.as_str(), *generation)));
            !generations.is_empty()
        });
    }
}

#[derive(Debug, Clone)]
struct FileLogShippingState {
    root: PathBuf,
    node_id: Uuid,
}

impl FileLogShippingState {
    fn new(root: impl Into<PathBuf>, node_id: Uuid) -> Result<Self, LogShippingError> {
        if node_id.is_nil() {
            return Err(LogShippingError::Invalid(
                "log shipping node ID must not be nil".into(),
            ));
        }
        Ok(Self {
            root: root.into(),
            node_id,
        })
    }

    async fn snapshot(
        &self,
        targets: Vec<RuntimeLogTarget>,
    ) -> Result<LogShippingState, LogShippingError> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || state.snapshot_sync(&targets))
            .await
            .map_err(task_error)?
    }

    async fn set_pending(&self, batch: NodeLogChunkBatch) -> Result<(), LogShippingError> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || state.set_pending_sync(batch))
            .await
            .map_err(task_error)?
    }

    async fn commit(&self, receipt: NodeLogChunkReceipt) -> Result<(), LogShippingError> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || state.commit_sync(receipt))
            .await
            .map_err(task_error)?
    }

    fn snapshot_sync(
        &self,
        targets: &[RuntimeLogTarget],
    ) -> Result<LogShippingState, LogShippingError> {
        state_file::ensure_directory(&self.root).map_err(state_error)?;
        let _lock =
            StateLock::exclusive(&self.root.join(LOG_SHIPPING_LOCK_FILE)).map_err(state_error)?;
        let mut state = self.read_state()?;
        if state.pending.is_none() {
            let original = state.cursors.clone();
            state.retain_targets(targets);
            if state.cursors != original {
                self.write_state(&state)?;
            }
        }
        Ok(state)
    }

    fn set_pending_sync(&self, batch: NodeLogChunkBatch) -> Result<(), LogShippingError> {
        batch.validate().map_err(LogShippingError::Invalid)?;
        if batch.node_id != self.node_id {
            return Err(LogShippingError::Invalid(
                "pending log batch belongs to another node".into(),
            ));
        }
        state_file::ensure_directory(&self.root).map_err(state_error)?;
        let _lock =
            StateLock::exclusive(&self.root.join(LOG_SHIPPING_LOCK_FILE)).map_err(state_error)?;
        let mut state = self.read_state()?;
        if state.pending.is_some() {
            return Err(LogShippingError::Conflict(
                "a durable log batch is already pending".into(),
            ));
        }
        state.pending = Some(batch);
        state.validate(self.node_id)?;
        self.write_state(&state)
    }

    fn commit_sync(&self, receipt: NodeLogChunkReceipt) -> Result<(), LogShippingError> {
        receipt.validate().map_err(LogShippingError::Invalid)?;
        state_file::ensure_directory(&self.root).map_err(state_error)?;
        let _lock =
            StateLock::exclusive(&self.root.join(LOG_SHIPPING_LOCK_FILE)).map_err(state_error)?;
        let mut state = self.read_state()?;
        let pending = state.pending.as_ref().ok_or_else(|| {
            LogShippingError::Conflict("log receipt has no durable pending batch".into())
        })?;
        if receipt.batch_id != pending.batch_id
            || receipt.node_id != pending.node_id
            || usize::from(receipt.accepted_chunks) != pending.chunks.len()
        {
            return Err(LogShippingError::Invalid(
                "log receipt changed the pending batch identity or chunk count".into(),
            ));
        }
        for report in &pending.chunks {
            state
                .cursors
                .entry(report.unit_id.clone())
                .or_default()
                .insert(
                    report.generation,
                    DurableLogCursor {
                        cursor: report.chunk.cursor.clone(),
                        sequence: report.chunk.sequence,
                    },
                );
        }
        state.pending = None;
        state.validate(self.node_id)?;
        self.write_state(&state)
    }

    fn read_state(&self) -> Result<LogShippingState, LogShippingError> {
        let path = self.root.join(LOG_SHIPPING_FILE);
        let state = state_file::read_json(&path, "node log shipping state")
            .map_err(state_error)?
            .unwrap_or_else(|| LogShippingState::empty(self.node_id));
        state.validate(self.node_id)?;
        Ok(state)
    }

    fn write_state(&self, state: &LogShippingState) -> Result<(), LogShippingError> {
        state_file::atomic_write(&self.root.join(LOG_SHIPPING_FILE), state).map_err(state_error)
    }
}

pub(crate) struct LogShipper {
    node_id: Uuid,
    runtime: Arc<dyn RuntimeClient>,
    transport: Arc<dyn NodeControlTransport>,
    state: FileLogShippingState,
    config: LogShippingConfig,
}

impl LogShipper {
    pub(crate) fn new(
        node_id: Uuid,
        runtime: Arc<dyn RuntimeClient>,
        transport: Arc<dyn NodeControlTransport>,
        state_dir: PathBuf,
        config: LogShippingConfig,
    ) -> Result<Self, LogShippingError> {
        validate_config(&config)?;
        Ok(Self {
            node_id,
            runtime,
            transport,
            state: FileLogShippingState::new(state_dir, node_id)?,
            config,
        })
    }

    pub(crate) async fn ship_once(
        &self,
        targets: &[RuntimeLogTarget],
    ) -> Result<bool, LogShippingError> {
        validate_targets(targets)?;
        let snapshot = self.state.snapshot(targets.to_vec()).await?;
        if let Some(pending) = snapshot.pending {
            self.upload(pending).await?;
            return Ok(true);
        }
        let Some(batch) = self.collect(targets, &snapshot).await? else {
            return Ok(false);
        };
        self.state.set_pending(batch.clone()).await?;
        self.upload(batch).await?;
        Ok(true)
    }

    async fn upload(&self, batch: NodeLogChunkBatch) -> Result<(), LogShippingError> {
        let receipt = self.transport.record_log_chunks(&batch).await?;
        self.state.commit(receipt).await
    }

    async fn collect(
        &self,
        targets: &[RuntimeLogTarget],
        snapshot: &LogShippingState,
    ) -> Result<Option<NodeLogChunkBatch>, LogShippingError> {
        let maximum_chunks = usize::from(self.config.max_batch_chunks);
        let mut reports = Vec::with_capacity(maximum_chunks);
        let mut data_bytes = 0_usize;
        'targets: for target in targets {
            let remaining = maximum_chunks.saturating_sub(reports.len());
            if remaining == 0 || data_bytes == self.config.max_batch_bytes {
                break;
            }
            let durable = snapshot.cursor(&target.unit_id, target.generation);
            let query = RuntimeLogQuery {
                schema: RuntimeLogQuery::SCHEMA.into(),
                unit_id: target.unit_id.clone(),
                generation: target.generation,
                cursor: durable.map(|cursor| cursor.cursor.clone()),
                limit: u32::try_from(remaining).map_err(|_| {
                    LogShippingError::Invalid("log batch chunk bound overflowed".into())
                })?,
                stream: None,
            };
            query.validate().map_err(LogShippingError::Invalid)?;
            let chunks = match self.runtime.logs(&query).await {
                Ok(chunks) => chunks,
                Err(RuntimeError::NotFound { .. }) => continue,
                Err(error) => return Err(error.into()),
            };
            if chunks.len() > remaining {
                return Err(LogShippingError::Invalid(
                    "Runtime returned more log chunks than requested".into(),
                ));
            }
            let mut previous_sequence = durable.map(|cursor| cursor.sequence);
            let mut cursors = BTreeSet::new();
            for chunk in chunks {
                chunk.validate().map_err(LogShippingError::Invalid)?;
                if previous_sequence.is_some_and(|sequence| chunk.sequence <= sequence)
                    || !cursors.insert(chunk.cursor.clone())
                {
                    return Err(LogShippingError::Invalid(
                        "Runtime log chunks do not strictly advance sequence and cursor".into(),
                    ));
                }
                let next_data_bytes =
                    data_bytes.checked_add(chunk.data.len()).ok_or_else(|| {
                        LogShippingError::Invalid("log batch byte count overflowed".into())
                    })?;
                if next_data_bytes > self.config.max_batch_bytes {
                    break 'targets;
                }
                previous_sequence = Some(chunk.sequence);
                data_bytes = next_data_bytes;
                let checksum = format!("sha256:{:x}", Sha256::digest(chunk.data.as_bytes()));
                reports.push(NodeLogChunkReport {
                    unit_id: target.unit_id.clone(),
                    generation: target.generation,
                    chunk,
                    checksum,
                });
                if reports.len() == maximum_chunks {
                    break 'targets;
                }
            }
        }
        if reports.is_empty() {
            return Ok(None);
        }
        let batch = NodeLogChunkBatch {
            schema: NodeLogChunkBatch::SCHEMA.into(),
            batch_id: Uuid::now_v7(),
            node_id: self.node_id,
            sent_at: Utc::now(),
            chunks: reports,
        };
        batch.validate().map_err(LogShippingError::Invalid)?;
        Ok(Some(batch))
    }
}

fn validate_config(config: &LogShippingConfig) -> Result<(), LogShippingError> {
    if config.poll_interval_ms == 0
        || config.poll_interval_ms > 60_000
        || config.max_batch_chunks == 0
        || config.max_batch_chunks > 256
        || !(1024 * 1024..=16 * 1024 * 1024).contains(&config.max_batch_bytes)
    {
        return Err(LogShippingError::Invalid(
            "log shipping configuration is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_targets(targets: &[RuntimeLogTarget]) -> Result<(), LogShippingError> {
    let mut identities = BTreeSet::new();
    for target in targets {
        validate_target(&target.unit_id, target.generation)?;
        if !identities.insert((target.unit_id.as_str(), target.generation)) {
            return Err(LogShippingError::Invalid(
                "log target list contains duplicates".into(),
            ));
        }
    }
    Ok(())
}

fn validate_target(unit_id: &str, generation: u64) -> Result<(), LogShippingError> {
    if unit_id.is_empty() || unit_id.len() > 512 || unit_id.contains('\0') || generation == 0 {
        return Err(LogShippingError::Invalid(
            "Runtime log target is invalid".into(),
        ));
    }
    Ok(())
}

fn state_error(error: state_file::SecureStateError) -> LogShippingError {
    LogShippingError::State(error.to_string())
}

fn task_error(error: tokio::task::JoinError) -> LogShippingError {
    LogShippingError::State(format!("log shipping state task failed: {error}"))
}

#[derive(Debug, thiserror::Error)]
pub enum LogShippingError {
    #[error("invalid log shipping data: {0}")]
    Invalid(String),
    #[error("log shipping state conflict: {0}")]
    Conflict(String),
    #[error("log shipping state failed: {0}")]
    State(String),
    #[error(transparent)]
    ControlPlane(#[from] NodeControlClientError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}

impl LogShippingError {
    pub fn retryable(&self) -> bool {
        match self {
            Self::ControlPlane(error) => error.retryable(),
            Self::Runtime(RuntimeError::ProviderUnavailable(_) | RuntimeError::Transport(_)) => {
                true
            }
            Self::Invalid(_) | Self::Conflict(_) | Self::State(_) | Self::Runtime(_) => false,
        }
    }
}

#[cfg(test)]
#[path = "log_shipper_tests.rs"]
mod tests;
