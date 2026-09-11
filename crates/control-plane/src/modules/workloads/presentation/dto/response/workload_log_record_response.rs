use crate::modules::workloads::application::{WorkloadLogGapReason, WorkloadLogRecord};
use crate::presentation::SequenceRecord;
use a3s_runtime::contract::RuntimeLogStream;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadLogRecordKind {
    Data,
    Gap,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadLogRecordResponse {
    pub kind: WorkloadLogRecordKind,
    pub source_cursor: Option<String>,
    pub sequence: u64,
    pub observed_at_ms: Option<u64>,
    pub stream: Option<&'static str>,
    pub data: Option<String>,
    pub gap_reason: Option<&'static str>,
    pub from_sequence: Option<u64>,
    pub through_sequence: Option<u64>,
    pub compacted_chunks: Option<u64>,
}

impl SequenceRecord for WorkloadLogRecordResponse {
    fn sequence(&self) -> u64 {
        self.sequence
    }
}

impl From<WorkloadLogRecord> for WorkloadLogRecordResponse {
    fn from(record: WorkloadLogRecord) -> Self {
        match record {
            WorkloadLogRecord::Data(chunk) => Self {
                kind: WorkloadLogRecordKind::Data,
                source_cursor: Some(chunk.cursor),
                sequence: chunk.sequence,
                observed_at_ms: Some(chunk.observed_at_ms),
                stream: Some(stream_name(chunk.stream)),
                data: Some(chunk.data),
                gap_reason: None,
                from_sequence: None,
                through_sequence: None,
                compacted_chunks: None,
            },
            WorkloadLogRecord::Gap { metadata, reason } => Self {
                kind: WorkloadLogRecordKind::Gap,
                source_cursor: Some(metadata.cursor),
                sequence: metadata.sequence,
                observed_at_ms: Some(metadata.observed_at_ms),
                stream: Some(stream_name(metadata.stream)),
                data: None,
                gap_reason: Some(gap_reason_name(reason)),
                from_sequence: None,
                through_sequence: None,
                compacted_chunks: None,
            },
            WorkloadLogRecord::CompactedGap { range } => Self {
                kind: WorkloadLogRecordKind::Gap,
                source_cursor: None,
                sequence: range.through_sequence,
                observed_at_ms: None,
                stream: None,
                data: None,
                gap_reason: Some(gap_reason_name(WorkloadLogGapReason::Compacted)),
                from_sequence: Some(range.first_sequence),
                through_sequence: Some(range.through_sequence),
                compacted_chunks: Some(range.compacted_chunks),
            },
            WorkloadLogRecord::ProviderGap { metadata } => Self {
                kind: WorkloadLogRecordKind::Gap,
                source_cursor: metadata.cursor,
                sequence: metadata.sequence,
                observed_at_ms: Some(metadata.observed_at_ms),
                stream: None,
                data: None,
                gap_reason: Some(gap_reason_name(metadata.reason)),
                from_sequence: None,
                through_sequence: None,
                compacted_chunks: None,
            },
        }
    }
}

const fn gap_reason_name(reason: WorkloadLogGapReason) -> &'static str {
    match reason {
        WorkloadLogGapReason::Missing => "missing",
        WorkloadLogGapReason::Corrupt => "corrupt",
        WorkloadLogGapReason::Retained => "retained",
        WorkloadLogGapReason::Compacted => "compacted",
        WorkloadLogGapReason::ProviderCursorLost => "provider_cursor_lost",
        WorkloadLogGapReason::ProviderDisconnected => "provider_disconnected",
    }
}

const fn stream_name(stream: RuntimeLogStream) -> &'static str {
    match stream {
        RuntimeLogStream::Stdout => "stdout",
        RuntimeLogStream::Stderr => "stderr",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::workloads::application::{
        WorkloadLogChunkMetadata, WorkloadLogCompactionRange, WorkloadLogProviderGapMetadata,
    };
    use a3s_runtime::contract::RuntimeLogChunk;
    use serde_json::json;

    #[test]
    fn compacted_gap_json_has_explicit_range_and_nullable_source_fields() {
        let response = WorkloadLogRecordResponse::from(WorkloadLogRecord::CompactedGap {
            range: WorkloadLogCompactionRange {
                first_sequence: 4,
                through_sequence: 7,
                compacted_chunks: 4,
            },
        });

        assert_eq!(
            serde_json::to_value(response).expect("serialize compacted log gap"),
            json!({
                "kind": "gap",
                "sourceCursor": null,
                "sequence": 7,
                "observedAtMs": null,
                "stream": null,
                "data": null,
                "gapReason": "compacted",
                "fromSequence": 4,
                "throughSequence": 7,
                "compactedChunks": 4
            })
        );
    }

    #[test]
    fn provider_gap_json_preserves_the_exact_boundary_and_typed_reason() {
        let cursor_lost = WorkloadLogRecordResponse::from(WorkloadLogRecord::ProviderGap {
            metadata: WorkloadLogProviderGapMetadata {
                cursor: Some("provider-cursor".into()),
                sequence: 8,
                observed_at_ms: 1_000,
                reason: WorkloadLogGapReason::ProviderCursorLost,
            },
        });
        assert_eq!(
            serde_json::to_value(cursor_lost).expect("serialize provider cursor loss"),
            json!({
                "kind": "gap",
                "sourceCursor": "provider-cursor",
                "sequence": 8,
                "observedAtMs": 1_000,
                "stream": null,
                "data": null,
                "gapReason": "provider_cursor_lost",
                "fromSequence": null,
                "throughSequence": null,
                "compactedChunks": null
            })
        );

        let disconnected = WorkloadLogRecordResponse::from(WorkloadLogRecord::ProviderGap {
            metadata: WorkloadLogProviderGapMetadata {
                cursor: None,
                sequence: 9,
                observed_at_ms: 1_001,
                reason: WorkloadLogGapReason::ProviderDisconnected,
            },
        });
        assert_eq!(
            serde_json::to_value(disconnected).expect("serialize provider disconnect"),
            json!({
                "kind": "gap",
                "sourceCursor": null,
                "sequence": 9,
                "observedAtMs": 1_001,
                "stream": null,
                "data": null,
                "gapReason": "provider_disconnected",
                "fromSequence": null,
                "throughSequence": null,
                "compactedChunks": null
            })
        );
    }

    #[test]
    fn data_and_gap_json_preserve_chunk_identity() {
        let data = WorkloadLogRecordResponse::from(WorkloadLogRecord::Data(RuntimeLogChunk {
            schema: RuntimeLogChunk::SCHEMA.into(),
            cursor: "c1".into(),
            sequence: 1,
            observed_at_ms: 10,
            stream: RuntimeLogStream::Stdout,
            data: "line\n".into(),
        }));
        assert_eq!(
            serde_json::to_value(data).expect("serialize data"),
            json!({
                "kind": "data",
                "sourceCursor": "c1",
                "sequence": 1,
                "observedAtMs": 10,
                "stream": "stdout",
                "data": "line\n",
                "gapReason": null,
                "fromSequence": null,
                "throughSequence": null,
                "compactedChunks": null
            })
        );

        let gap = WorkloadLogRecordResponse::from(WorkloadLogRecord::Gap {
            metadata: WorkloadLogChunkMetadata {
                cursor: "c2".into(),
                sequence: 2,
                observed_at_ms: 11,
                stream: RuntimeLogStream::Stderr,
            },
            reason: WorkloadLogGapReason::Missing,
        });
        assert_eq!(
            serde_json::to_value(gap).expect("serialize gap"),
            json!({
                "kind": "gap",
                "sourceCursor": "c2",
                "sequence": 2,
                "observedAtMs": 11,
                "stream": "stderr",
                "data": null,
                "gapReason": "missing",
                "fromSequence": null,
                "throughSequence": null,
                "compactedChunks": null
            })
        );
    }
}
