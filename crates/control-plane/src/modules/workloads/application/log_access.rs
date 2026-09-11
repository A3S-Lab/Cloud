use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::NodeId;
use crate::modules::workloads::application::queries::WorkloadLogRecord;
use a3s_runtime::contract::RuntimeLogStream;
use async_trait::async_trait;

/// Workloads-owned upper bound for one log page. Matches Fleet's published
/// reader limit so callers keep a stable contract without importing Fleet.
pub const WORKLOAD_MAX_LOG_PAGE_SIZE: u16 = 256;

/// Exact Runtime log window Workloads is willing to read through Fleet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadLogReadQuery {
    pub node_id: NodeId,
    pub unit_id: String,
    pub generation: u64,
    pub after_sequence: Option<u64>,
    pub limit: u16,
    pub stream: Option<RuntimeLogStream>,
}

/// Aggregate-free log page returned by the Workloads log access port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadLogReadResult {
    pub node_id: NodeId,
    pub unit_id: String,
    pub generation: u64,
    pub records: Vec<WorkloadLogRecord>,
    pub next_after_sequence: Option<u64>,
}

/// Workloads-owned read port for Runtime log pages.
#[async_trait]
pub trait IWorkloadLogAccess: Send + Sync {
    async fn read(&self, query: WorkloadLogReadQuery) -> ApplicationResult<WorkloadLogReadResult>;
}
