use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, NodeLogChunkMetadata, NodeLogChunkQuery, NodeLogCompactionRange,
    NodeLogGapMetadata,
};
use crate::modules::fleet::domain::services::{
    ILogChunkStore, LogChunkStoreError, RetrievedLogChunk,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::NodeId;
use a3s_cloud_contracts::NodeLogChunkReport;
use a3s_runtime::contract::{RuntimeLogChunk, RuntimeLogStream};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeLogGapReason {
    Missing,
    Corrupt,
    Retained,
    Compacted,
    ProviderCursorLost,
    ProviderDisconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeLogRecord {
    Data(RuntimeLogChunk),
    Gap {
        metadata: NodeLogChunkMetadata,
        reason: NodeLogGapReason,
    },
    CompactedGap {
        range: NodeLogCompactionRange,
    },
    ProviderGap {
        metadata: NodeLogGapMetadata,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLogPage {
    pub node_id: NodeId,
    pub unit_id: String,
    pub generation: u64,
    pub records: Vec<NodeLogRecord>,
    pub next_after_sequence: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLogReadQuery {
    pub node_id: NodeId,
    pub unit_id: String,
    pub generation: u64,
    pub after_sequence: Option<u64>,
    pub limit: u16,
    pub stream: Option<RuntimeLogStream>,
}

#[derive(Clone)]
pub struct NodeLogReader {
    metadata: Arc<dyn INodeControlRepository>,
    objects: Arc<dyn ILogChunkStore>,
}

impl NodeLogReader {
    pub fn new(
        metadata: Arc<dyn INodeControlRepository>,
        objects: Arc<dyn ILogChunkStore>,
    ) -> Self {
        Self { metadata, objects }
    }

    pub async fn read(&self, query: NodeLogReadQuery) -> ApplicationResult<NodeLogPage> {
        if query.limit == 0 || query.limit > 256 {
            return Err(ApplicationError::Invalid(
                "log limit must be between 1 and 256".into(),
            ));
        }
        let fetch_limit = usize::from(query.limit) + 1;
        let metadata_query = NodeLogChunkQuery {
            node_id: query.node_id,
            unit_id: query.unit_id.clone(),
            generation: query.generation,
            after_sequence: query.after_sequence,
            limit: fetch_limit,
            stream: query.stream,
        };
        let mut chunks = self
            .metadata
            .list_log_chunks(metadata_query.clone())
            .await
            .map_err(ApplicationError::from)?;
        let gaps = self
            .metadata
            .list_log_gaps(metadata_query.clone())
            .await
            .map_err(ApplicationError::from)?;
        let ranges = self
            .metadata
            .list_log_compaction_ranges(metadata_query)
            .await
            .map_err(ApplicationError::from)?;
        let source_has_more = chunks.len() > usize::from(query.limit)
            || gaps.len() > usize::from(query.limit)
            || ranges.len() > usize::from(query.limit);
        chunks.retain(|chunk| {
            !ranges.iter().any(|range| {
                (range.first_sequence..=range.through_sequence).contains(&chunk.sequence)
            })
        });
        let mut stored = chunks
            .into_iter()
            .map(StoredLogRecord::Chunk)
            .chain(gaps.into_iter().map(StoredLogRecord::ProviderGap))
            .chain(ranges.into_iter().map(StoredLogRecord::Compacted))
            .collect::<Vec<_>>();
        stored.sort_by_key(StoredLogRecord::first_sequence);
        let has_more = source_has_more || stored.len() > usize::from(query.limit);
        if has_more {
            stored.truncate(usize::from(query.limit));
        }
        let next_after_sequence = has_more
            .then(|| stored.last().map(StoredLogRecord::through_sequence))
            .flatten();
        let mut records = Vec::with_capacity(stored.len());
        for stored in stored {
            let chunk = match stored {
                StoredLogRecord::Chunk(chunk) => chunk,
                StoredLogRecord::Compacted(range) => {
                    records.push(NodeLogRecord::CompactedGap { range });
                    continue;
                }
                StoredLogRecord::ProviderGap(metadata) => {
                    records.push(NodeLogRecord::ProviderGap { metadata });
                    continue;
                }
            };
            if chunk.retained_at.is_some() {
                records.push(NodeLogRecord::Gap {
                    metadata: chunk,
                    reason: NodeLogGapReason::Retained,
                });
                continue;
            }
            let object = match self.objects.get(&chunk.object_key, &chunk.checksum).await {
                Ok(object) => object,
                Err(LogChunkStoreError::Unavailable(_)) => {
                    return Err(ApplicationError::Internal(
                        "log object storage is unavailable".into(),
                    ));
                }
                Err(LogChunkStoreError::Invalid(_) | LogChunkStoreError::Conflict(_)) => {
                    RetrievedLogChunk::Corrupt
                }
            };
            records.push(match object {
                RetrievedLogChunk::Found(report) if report_matches(&report, &chunk) => {
                    NodeLogRecord::Data(report.chunk)
                }
                RetrievedLogChunk::Found(_) | RetrievedLogChunk::Corrupt => NodeLogRecord::Gap {
                    metadata: chunk,
                    reason: NodeLogGapReason::Corrupt,
                },
                RetrievedLogChunk::Missing => NodeLogRecord::Gap {
                    metadata: chunk,
                    reason: NodeLogGapReason::Missing,
                },
            });
        }
        Ok(NodeLogPage {
            node_id: query.node_id,
            unit_id: query.unit_id,
            generation: query.generation,
            records,
            next_after_sequence,
        })
    }
}

fn report_matches(report: &NodeLogChunkReport, metadata: &NodeLogChunkMetadata) -> bool {
    report.unit_id == metadata.unit_id
        && report.generation == metadata.generation
        && report.chunk.cursor == metadata.cursor
        && report.chunk.sequence == metadata.sequence
        && report.chunk.observed_at_ms == metadata.observed_at_ms
        && report.chunk.stream == metadata.stream
        && report.checksum == metadata.checksum
}

enum StoredLogRecord {
    Chunk(NodeLogChunkMetadata),
    ProviderGap(NodeLogGapMetadata),
    Compacted(NodeLogCompactionRange),
}

impl StoredLogRecord {
    const fn first_sequence(&self) -> u64 {
        match self {
            Self::Chunk(chunk) => chunk.sequence,
            Self::ProviderGap(gap) => gap.sequence,
            Self::Compacted(range) => range.first_sequence,
        }
    }

    const fn through_sequence(&self) -> u64 {
        match self {
            Self::Chunk(chunk) => chunk.sequence,
            Self::ProviderGap(gap) => gap.sequence,
            Self::Compacted(range) => range.through_sequence,
        }
    }
}
