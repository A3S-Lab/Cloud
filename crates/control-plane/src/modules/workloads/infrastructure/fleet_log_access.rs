use crate::modules::fleet::application::{
    NodeLogGapReason, NodeLogReadQuery, NodeLogReader, NodeLogRecord,
};
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::fleet::domain::services::ILogChunkStore;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::workloads::application::{
    IWorkloadLogAccess, WorkloadLogChunkMetadata, WorkloadLogCompactionRange, WorkloadLogGapReason,
    WorkloadLogProviderGapMetadata, WorkloadLogReadQuery, WorkloadLogReadResult, WorkloadLogRecord,
};
use a3s_runtime::contract::RuntimeLogDiscontinuityReason;
use async_trait::async_trait;
use std::sync::Arc;

/// Read-only anti-corruption adapter for Fleet Runtime log pages.
#[derive(Clone)]
pub struct FleetWorkloadLogAccessAdapter {
    logs: NodeLogReader,
}

impl FleetWorkloadLogAccessAdapter {
    pub fn new(
        metadata: Arc<dyn INodeControlRepository>,
        objects: Arc<dyn ILogChunkStore>,
    ) -> Self {
        Self {
            logs: NodeLogReader::new(metadata, objects),
        }
    }
}

#[async_trait]
impl IWorkloadLogAccess for FleetWorkloadLogAccessAdapter {
    async fn read(&self, query: WorkloadLogReadQuery) -> ApplicationResult<WorkloadLogReadResult> {
        let page = self
            .logs
            .read(NodeLogReadQuery {
                node_id: query.node_id,
                unit_id: query.unit_id,
                generation: query.generation,
                after_sequence: query.after_sequence,
                limit: query.limit,
                stream: query.stream,
            })
            .await?;
        Ok(WorkloadLogReadResult {
            node_id: page.node_id,
            unit_id: page.unit_id,
            generation: page.generation,
            records: page
                .records
                .into_iter()
                .map(project_log_record)
                .collect(),
            next_after_sequence: page.next_after_sequence,
        })
    }
}

fn project_log_record(record: NodeLogRecord) -> WorkloadLogRecord {
    match record {
        NodeLogRecord::Data(chunk) => WorkloadLogRecord::Data(chunk),
        NodeLogRecord::Gap { metadata, reason } => WorkloadLogRecord::Gap {
            metadata: WorkloadLogChunkMetadata {
                cursor: metadata.cursor,
                sequence: metadata.sequence,
                observed_at_ms: metadata.observed_at_ms,
                stream: metadata.stream,
            },
            reason: project_gap_reason(reason),
        },
        NodeLogRecord::CompactedGap { range } => WorkloadLogRecord::CompactedGap {
            range: WorkloadLogCompactionRange {
                first_sequence: range.first_sequence,
                through_sequence: range.through_sequence,
                compacted_chunks: range.compacted_chunks(),
            },
        },
        NodeLogRecord::ProviderGap { metadata } => WorkloadLogRecord::ProviderGap {
            metadata: WorkloadLogProviderGapMetadata {
                cursor: metadata.cursor,
                sequence: metadata.sequence,
                observed_at_ms: metadata.observed_at_ms,
                reason: project_provider_gap_reason(metadata.reason),
            },
        },
    }
}

const fn project_gap_reason(reason: NodeLogGapReason) -> WorkloadLogGapReason {
    match reason {
        NodeLogGapReason::Missing => WorkloadLogGapReason::Missing,
        NodeLogGapReason::Corrupt => WorkloadLogGapReason::Corrupt,
        NodeLogGapReason::Retained => WorkloadLogGapReason::Retained,
        NodeLogGapReason::Compacted => WorkloadLogGapReason::Compacted,
        NodeLogGapReason::ProviderCursorLost => WorkloadLogGapReason::ProviderCursorLost,
        NodeLogGapReason::ProviderDisconnected => WorkloadLogGapReason::ProviderDisconnected,
    }
}

const fn project_provider_gap_reason(
    reason: RuntimeLogDiscontinuityReason,
) -> WorkloadLogGapReason {
    match reason {
        RuntimeLogDiscontinuityReason::CursorLost => WorkloadLogGapReason::ProviderCursorLost,
        RuntimeLogDiscontinuityReason::SourceDisconnected => {
            WorkloadLogGapReason::ProviderDisconnected
        }
    }
}
