use crate::control_plane::NodeControlTransport;
use crate::state_file::{self, SecureStateError, StateLock};
use crate::NodeControlClientError;
use a3s_cloud_contracts::{
    NodeInventoryReference, NodeResourceInventory, NodeResourceSlot, ResourceAllocation,
    ResourceKind, ResourceUnit,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

const INVENTORY_FILE: &str = "resource-inventory.json";
const INVENTORY_LOCK_FILE: &str = "resource-inventory.lock";

#[async_trait]
trait ResourceInventoryDetector: Send + Sync {
    async fn detect(&self) -> Result<Vec<NodeResourceSlot>, ResourceInventoryError>;
}

#[async_trait]
pub trait NodeResourceInventoryAuthority: Send + Sync {
    async fn current_resource_inventory(
        &self,
    ) -> Result<NodeResourceInventory, ResourceInventoryError>;
}

#[derive(Debug, Clone)]
struct HostResourceInventoryDetector {
    state_directory: PathBuf,
}

impl HostResourceInventoryDetector {
    fn new(state_directory: impl Into<PathBuf>) -> Self {
        Self {
            state_directory: state_directory.into(),
        }
    }
}

#[async_trait]
impl ResourceInventoryDetector for HostResourceInventoryDetector {
    async fn detect(&self) -> Result<Vec<NodeResourceSlot>, ResourceInventoryError> {
        let state_directory = self.state_directory.clone();
        tokio::task::spawn_blocking(move || detect_host_resources(&state_directory))
            .await
            .map_err(|error| {
                ResourceInventoryError::Detection(format!(
                    "host resource detection task failed: {error}"
                ))
            })?
    }
}

#[derive(Debug, Clone)]
struct FileResourceInventoryStore {
    root: PathBuf,
}

impl FileResourceInventoryStore {
    fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    async fn prepare(
        &self,
        node_id: Uuid,
        agent_instance_id: Uuid,
        observed_at: DateTime<Utc>,
        slots: Vec<NodeResourceSlot>,
    ) -> Result<NodeResourceInventory, ResourceInventoryError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            store.prepare_sync(node_id, agent_instance_id, observed_at, slots)
        })
        .await
        .map_err(|error| {
            ResourceInventoryError::Storage(format!(
                "resource inventory state task failed: {error}"
            ))
        })?
    }

    fn prepare_sync(
        &self,
        node_id: Uuid,
        agent_instance_id: Uuid,
        mut observed_at: DateTime<Utc>,
        mut slots: Vec<NodeResourceSlot>,
    ) -> Result<NodeResourceInventory, ResourceInventoryError> {
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(INVENTORY_LOCK_FILE))?;
        let path = self.root.join(INVENTORY_FILE);
        let existing = state_file::read_json::<InventoryRecord>(&path, "resource inventory")?
            .map(InventoryRecord::into_inventory)
            .transpose()?;
        if let Some(existing) = &existing {
            if existing.node_id != node_id || existing.agent_instance_id != agent_instance_id {
                return Err(ResourceInventoryError::Invalid(
                    "persisted resource inventory belongs to another node-agent identity".into(),
                ));
            }
            slots.sort_by(|left, right| {
                (left.kind, left.stable_resource_id.as_str())
                    .cmp(&(right.kind, right.stable_resource_id.as_str()))
            });
            if slots == existing.slots {
                return Ok(existing.clone());
            }
            if observed_at <= existing.observed_at {
                observed_at = existing
                    .observed_at
                    .checked_add_signed(Duration::microseconds(1))
                    .ok_or_else(|| {
                        ResourceInventoryError::Invalid(
                            "resource inventory observation time overflowed".into(),
                        )
                    })?;
            }
        }
        let generation = existing.as_ref().map_or(Ok(1), |inventory| {
            inventory.generation.checked_add(1).ok_or_else(|| {
                ResourceInventoryError::Invalid("resource inventory generation is exhausted".into())
            })
        })?;
        let inventory =
            NodeResourceInventory::new(node_id, agent_instance_id, generation, observed_at, slots)
                .map_err(ResourceInventoryError::Invalid)?;
        state_file::atomic_write(&path, &InventoryRecord::new(inventory.clone()))?;
        Ok(inventory)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InventoryRecord {
    schema: String,
    inventory: NodeResourceInventory,
}

impl InventoryRecord {
    const SCHEMA: &'static str = "a3s.cloud.node-resource-inventory-state.v1";

    fn new(inventory: NodeResourceInventory) -> Self {
        Self {
            schema: Self::SCHEMA.into(),
            inventory,
        }
    }

    fn into_inventory(self) -> Result<NodeResourceInventory, ResourceInventoryError> {
        if self.schema != Self::SCHEMA {
            return Err(ResourceInventoryError::Invalid(format!(
                "unsupported resource inventory state schema {:?}",
                self.schema
            )));
        }
        self.inventory
            .validate()
            .map_err(ResourceInventoryError::Invalid)?;
        Ok(self.inventory)
    }
}

pub(crate) struct ResourceInventoryManager {
    node_id: Uuid,
    agent_instance_id: Uuid,
    detector: Arc<dyn ResourceInventoryDetector>,
    store: FileResourceInventoryStore,
    transport: Arc<dyn NodeControlTransport>,
    confirmed: Mutex<Option<NodeInventoryReference>>,
}

impl ResourceInventoryManager {
    pub(crate) fn host(
        node_id: Uuid,
        agent_instance_id: Uuid,
        state_directory: PathBuf,
        transport: Arc<dyn NodeControlTransport>,
    ) -> Self {
        Self {
            node_id,
            agent_instance_id,
            detector: Arc::new(HostResourceInventoryDetector::new(&state_directory)),
            store: FileResourceInventoryStore::new(state_directory),
            transport,
            confirmed: Mutex::new(None),
        }
    }

    pub(crate) async fn ensure_reported(
        &self,
    ) -> Result<NodeInventoryReference, ResourceInventoryError> {
        Ok(self.ensure_reported_inventory().await?.reference())
    }

    async fn ensure_reported_inventory(
        &self,
    ) -> Result<NodeResourceInventory, ResourceInventoryError> {
        let mut confirmed = self.confirmed.lock().await;
        let slots = self.detector.detect().await?;
        let inventory = self
            .store
            .prepare(self.node_id, self.agent_instance_id, Utc::now(), slots)
            .await?;
        let reference = inventory.reference();
        if confirmed.as_ref() != Some(&reference) {
            let receipt = self.transport.report_resource_inventory(&inventory).await?;
            receipt
                .validate()
                .map_err(ResourceInventoryError::Invalid)?;
            if receipt.node_id != inventory.node_id
                || receipt.generation != inventory.generation
                || receipt.digest != inventory.digest
            {
                return Err(ResourceInventoryError::Invalid(
                    "resource inventory receipt changed the snapshot identity".into(),
                ));
            }
            *confirmed = Some(reference.clone());
        }
        Ok(inventory)
    }
}

#[async_trait]
impl NodeResourceInventoryAuthority for ResourceInventoryManager {
    async fn current_resource_inventory(
        &self,
    ) -> Result<NodeResourceInventory, ResourceInventoryError> {
        self.ensure_reported_inventory().await
    }
}

fn detect_host_resources(
    state_directory: &Path,
) -> Result<Vec<NodeResourceSlot>, ResourceInventoryError> {
    state_file::ensure_directory(state_directory)?;
    let parallelism = std::thread::available_parallelism()
        .map_err(|error| {
            ResourceInventoryError::Detection(format!(
                "could not detect available CPU parallelism: {error}"
            ))
        })?
        .get();
    let milli_cpu = u64::try_from(parallelism)
        .ok()
        .and_then(|value| value.checked_mul(1_000))
        .ok_or_else(|| {
            ResourceInventoryError::Detection("available CPU capacity overflowed".into())
        })?;
    let mut slots = vec![
        NodeResourceSlot::new(
            ResourceKind::Cpu,
            "cpu/shared",
            ResourceAllocation::Scalar {
                amount: milli_cpu,
                unit: ResourceUnit::MilliCpu,
            },
        )
        .map_err(ResourceInventoryError::Invalid)?,
        NodeResourceSlot::new(
            ResourceKind::EphemeralStorage,
            "ephemeral-storage/state-filesystem",
            ResourceAllocation::Scalar {
                amount: fs2::total_space(state_directory).map_err(|error| {
                    ResourceInventoryError::Detection(format!(
                        "could not detect state filesystem capacity: {error}"
                    ))
                })?,
                unit: ResourceUnit::Byte,
            },
        )
        .map_err(ResourceInventoryError::Invalid)?,
    ];
    if let Some(memory_bytes) = detect_memory_bytes()? {
        slots.push(
            NodeResourceSlot::new(
                ResourceKind::Memory,
                "memory/system",
                ResourceAllocation::Scalar {
                    amount: memory_bytes,
                    unit: ResourceUnit::Byte,
                },
            )
            .map_err(ResourceInventoryError::Invalid)?,
        );
    }
    slots.sort_by(|left, right| {
        (left.kind, left.stable_resource_id.as_str())
            .cmp(&(right.kind, right.stable_resource_id.as_str()))
    });
    Ok(slots)
}

#[cfg(target_os = "linux")]
fn detect_memory_bytes() -> Result<Option<u64>, ResourceInventoryError> {
    let contents = std::fs::read_to_string("/proc/meminfo").map_err(|error| {
        ResourceInventoryError::Detection(format!("could not read /proc/meminfo: {error}"))
    })?;
    parse_linux_memory_bytes(&contents).map(Some)
}

#[cfg(not(target_os = "linux"))]
fn detect_memory_bytes() -> Result<Option<u64>, ResourceInventoryError> {
    Ok(None)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_linux_memory_bytes(contents: &str) -> Result<u64, ResourceInventoryError> {
    let line = contents
        .lines()
        .find(|line| line.starts_with("MemTotal:"))
        .ok_or_else(|| {
            ResourceInventoryError::Detection("/proc/meminfo omitted MemTotal".into())
        })?;
    let mut fields = line.split_ascii_whitespace();
    if fields.next() != Some("MemTotal:") {
        return Err(ResourceInventoryError::Detection(
            "/proc/meminfo MemTotal label is invalid".into(),
        ));
    }
    let kibibytes = fields
        .next()
        .ok_or_else(|| {
            ResourceInventoryError::Detection("/proc/meminfo MemTotal value is missing".into())
        })?
        .parse::<u64>()
        .map_err(|error| {
            ResourceInventoryError::Detection(format!(
                "/proc/meminfo MemTotal value is invalid: {error}"
            ))
        })?;
    if fields.next() != Some("kB") || fields.next().is_some() || kibibytes == 0 {
        return Err(ResourceInventoryError::Detection(
            "/proc/meminfo MemTotal unit or value is invalid".into(),
        ));
    }
    kibibytes.checked_mul(1_024).ok_or_else(|| {
        ResourceInventoryError::Detection("/proc/meminfo MemTotal overflowed".into())
    })
}

#[derive(Debug, thiserror::Error)]
pub enum ResourceInventoryError {
    #[error("invalid resource inventory: {0}")]
    Invalid(String),
    #[error("resource inventory detection failed: {0}")]
    Detection(String),
    #[error("resource inventory storage failed: {0}")]
    Storage(String),
    #[error(transparent)]
    ControlPlane(#[from] NodeControlClientError),
}

impl From<SecureStateError> for ResourceInventoryError {
    fn from(error: SecureStateError) -> Self {
        Self::Storage(error.to_string())
    }
}

impl ResourceInventoryError {
    pub fn retryable(&self) -> bool {
        matches!(self, Self::ControlPlane(error) if error.retryable())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_memory_parser_uses_stable_total_capacity() {
        assert_eq!(
            parse_linux_memory_bytes(
                "MemTotal:       16384 kB\nMemFree:         1024 kB\nMemAvailable:    2048 kB\n"
            )
            .expect("memory capacity"),
            16 * 1024 * 1024
        );
        assert!(parse_linux_memory_bytes("MemAvailable: 2048 kB\n").is_err());
        assert!(parse_linux_memory_bytes("MemTotal: 0 kB\n").is_err());
    }

    #[tokio::test]
    async fn file_store_reuses_content_and_advances_changed_capacity_after_restart() {
        let directory = tempfile::tempdir().expect("inventory state");
        let store = FileResourceInventoryStore::new(directory.path());
        let node_id = Uuid::now_v7();
        let agent_instance_id = Uuid::now_v7();
        let at = Utc::now();
        let slots = vec![NodeResourceSlot::new(
            ResourceKind::Cpu,
            "cpu/shared",
            ResourceAllocation::Scalar {
                amount: 2_000,
                unit: ResourceUnit::MilliCpu,
            },
        )
        .expect("CPU slot")];
        let first = store
            .prepare(node_id, agent_instance_id, at, slots.clone())
            .await
            .expect("first inventory");
        let replay = FileResourceInventoryStore::new(directory.path())
            .prepare(node_id, agent_instance_id, at + Duration::seconds(1), slots)
            .await
            .expect("replayed inventory");
        assert_eq!(replay, first);

        let changed = store
            .prepare(
                node_id,
                agent_instance_id,
                at + Duration::seconds(2),
                vec![NodeResourceSlot::new(
                    ResourceKind::Cpu,
                    "cpu/shared",
                    ResourceAllocation::Scalar {
                        amount: 3_000,
                        unit: ResourceUnit::MilliCpu,
                    },
                )
                .expect("changed CPU slot")],
            )
            .await
            .expect("changed inventory");
        assert_eq!(changed.generation, 2);
        assert_ne!(changed.digest, first.digest);
    }
}
