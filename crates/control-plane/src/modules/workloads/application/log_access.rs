use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::NodeId;
use a3s_runtime::contract::{RuntimeLogChunk, RuntimeLogStream};
use async_trait::async_trait;

/// Workloads-owned upper bound for one log page. Matches Fleet's published
/// reader limit so callers keep a stable contract without importing Fleet.
pub const WORKLOAD_MAX_LOG_PAGE_SIZE: u16 = 256;

/// Workloads-owned gap classification for Runtime log pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadLogGapReason {
    Missing,
    Corrupt,
    Retained,
    Compacted,
    ProviderCursorLost,
    ProviderDisconnected,
}

/// Exact chunk identity retained for one explicit gap in a Workload log page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadLogChunkMetadata {
    pub cursor: String,
    pub sequence: u64,
    pub observed_at_ms: u64,
    pub stream: RuntimeLogStream,
}

/// Exact compacted-sequence window retained for one Workload log gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadLogCompactionRange {
    pub first_sequence: u64,
    pub through_sequence: u64,
    pub compacted_chunks: u64,
}

/// Exact provider discontinuity retained for one Workload log gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadLogProviderGapMetadata {
    pub cursor: Option<String>,
    pub sequence: u64,
    pub observed_at_ms: u64,
    pub reason: WorkloadLogGapReason,
}

/// Workloads-owned Runtime log record projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadLogRecord {
    Data(RuntimeLogChunk),
    Gap {
        metadata: WorkloadLogChunkMetadata,
        reason: WorkloadLogGapReason,
    },
    CompactedGap {
        range: WorkloadLogCompactionRange,
    },
    ProviderGap {
        metadata: WorkloadLogProviderGapMetadata,
    },
}

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
