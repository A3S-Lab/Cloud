use crate::modules::artifacts::application::{
    BuildLogChunkGapReason, BuildLogRecord, BuildLogSourceGapReason, BuildLogStream,
    BuildRunLogPage,
};
use crate::presentation::{format_sequence_cursor, SequencePage, SequenceRecord};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildLogRecordKind {
    Data,
    Gap,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildLogRecordResponse {
    pub kind: BuildLogRecordKind,
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

impl SequenceRecord for BuildLogRecordResponse {
    fn sequence(&self) -> u64 {
        self.sequence
    }
}

impl From<BuildLogRecord> for BuildLogRecordResponse {
    fn from(record: BuildLogRecord) -> Self {
        match record {
            BuildLogRecord::Data(record) => Self {
                kind: BuildLogRecordKind::Data,
                source_cursor: Some(record.source_cursor().to_owned()),
                sequence: record.sequence(),
                observed_at_ms: Some(record.observed_at_ms()),
                stream: Some(stream_name(record.stream())),
                data: Some(record.data().to_owned()),
                gap_reason: None,
                from_sequence: None,
                through_sequence: None,
                compacted_chunks: None,
            },
            BuildLogRecord::ChunkGap(record) => Self {
                kind: BuildLogRecordKind::Gap,
                source_cursor: Some(record.source_cursor().to_owned()),
                sequence: record.sequence(),
                observed_at_ms: Some(record.observed_at_ms()),
                stream: Some(stream_name(record.stream())),
                data: None,
                gap_reason: Some(chunk_gap_reason_name(record.reason())),
                from_sequence: None,
                through_sequence: None,
                compacted_chunks: None,
            },
            BuildLogRecord::Compacted(record) => Self {
                kind: BuildLogRecordKind::Gap,
                source_cursor: None,
                sequence: record.through_sequence(),
                observed_at_ms: None,
                stream: None,
                data: None,
                gap_reason: Some("compacted"),
                from_sequence: Some(record.from_sequence()),
                through_sequence: Some(record.through_sequence()),
                compacted_chunks: Some(record.compacted_chunks()),
            },
            BuildLogRecord::SourceGap(record) => Self {
                kind: BuildLogRecordKind::Gap,
                source_cursor: record.source_cursor().map(str::to_owned),
                sequence: record.sequence(),
                observed_at_ms: Some(record.observed_at_ms()),
                stream: None,
                data: None,
                gap_reason: Some(source_gap_reason_name(record.reason())),
                from_sequence: None,
                through_sequence: None,
                compacted_chunks: None,
            },
        }
    }
}

const fn chunk_gap_reason_name(reason: BuildLogChunkGapReason) -> &'static str {
    match reason {
        BuildLogChunkGapReason::Missing => "missing",
        BuildLogChunkGapReason::Corrupt => "corrupt",
        BuildLogChunkGapReason::Retained => "retained",
    }
}

const fn source_gap_reason_name(reason: BuildLogSourceGapReason) -> &'static str {
    match reason {
        BuildLogSourceGapReason::CursorLost => "provider_cursor_lost",
        BuildLogSourceGapReason::SourceDisconnected => "provider_disconnected",
    }
}

const fn stream_name(stream: BuildLogStream) -> &'static str {
    match stream {
        BuildLogStream::Stdout => "stdout",
        BuildLogStream::Stderr => "stderr",
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRunLogsResponse {
    pub build_run_id: Uuid,
    pub operation_id: Uuid,
    pub generation: u64,
    pub records: Vec<BuildLogRecordResponse>,
    pub next_cursor: Option<String>,
}

impl From<BuildRunLogPage> for BuildRunLogsResponse {
    fn from(page: BuildRunLogPage) -> Self {
        Self {
            build_run_id: page.build_run_id.as_uuid(),
            operation_id: page.operation_id.as_uuid(),
            generation: page.generation,
            records: page.records.into_iter().map(Into::into).collect(),
            next_cursor: page.next_after_sequence.map(format_sequence_cursor),
        }
    }
}

impl SequencePage for BuildRunLogsResponse {
    type Record = BuildLogRecordResponse;

    fn records(&self) -> &[Self::Record] {
        &self.records
    }

    fn take_records(&mut self) -> Vec<Self::Record> {
        std::mem::take(&mut self.records)
    }

    fn replace_records(&mut self, records: Vec<Self::Record>) {
        self.records = records;
    }

    fn set_next_cursor(&mut self, cursor: Option<String>) {
        self.next_cursor = cursor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::application::{
        BuildLogChunkGap, BuildLogCompactedRange, BuildLogData, BuildLogSourceGap,
    };
    use crate::modules::shared_kernel::domain::{BuildRunId, OperationId};
    use serde_json::json;

    #[test]
    fn build_log_response_hides_runtime_placement_identity() {
        let response = BuildRunLogsResponse::from(BuildRunLogPage {
            build_run_id: BuildRunId::new(),
            operation_id: OperationId::new(),
            generation: 1,
            records: Vec::new(),
            next_after_sequence: None,
        });
        let encoded = serde_json::to_value(response).expect("build logs response");
        assert!(encoded.get("buildRunId").is_some());
        assert!(encoded.get("operationId").is_some());
        assert!(encoded.get("nodeId").is_none());
        assert!(encoded.get("unitId").is_none());
    }

    #[test]
    fn build_log_response_preserves_the_existing_wire_schema_with_local_contracts() {
        let response = BuildRunLogsResponse::from(BuildRunLogPage {
            build_run_id: BuildRunId::new(),
            operation_id: OperationId::new(),
            generation: 2,
            records: vec![
                BuildLogRecord::Data(
                    BuildLogData::new(
                        "cursor-1".into(),
                        1,
                        1_000,
                        BuildLogStream::Stdout,
                        "compiled".into(),
                    )
                    .expect("valid data"),
                ),
                BuildLogRecord::ChunkGap(
                    BuildLogChunkGap::new(
                        "cursor-2".into(),
                        2,
                        1_001,
                        BuildLogStream::Stderr,
                        BuildLogChunkGapReason::Corrupt,
                    )
                    .expect("valid chunk gap"),
                ),
                BuildLogRecord::Compacted(
                    BuildLogCompactedRange::new(3, 5).expect("valid compacted range"),
                ),
                BuildLogRecord::SourceGap(
                    BuildLogSourceGap::new(
                        Some("provider-cursor".into()),
                        6,
                        1_002,
                        BuildLogSourceGapReason::CursorLost,
                    )
                    .expect("valid source gap"),
                ),
            ],
            next_after_sequence: Some(6),
        });
        let encoded = serde_json::to_value(response).expect("serialize build logs");

        assert_eq!(
            encoded.get("records"),
            Some(&json!([
                {
                    "kind": "data",
                    "sourceCursor": "cursor-1",
                    "sequence": 1,
                    "observedAtMs": 1_000,
                    "stream": "stdout",
                    "data": "compiled",
                    "gapReason": null,
                    "fromSequence": null,
                    "throughSequence": null,
                    "compactedChunks": null
                },
                {
                    "kind": "gap",
                    "sourceCursor": "cursor-2",
                    "sequence": 2,
                    "observedAtMs": 1_001,
                    "stream": "stderr",
                    "data": null,
                    "gapReason": "corrupt",
                    "fromSequence": null,
                    "throughSequence": null,
                    "compactedChunks": null
                },
                {
                    "kind": "gap",
                    "sourceCursor": null,
                    "sequence": 5,
                    "observedAtMs": null,
                    "stream": null,
                    "data": null,
                    "gapReason": "compacted",
                    "fromSequence": 3,
                    "throughSequence": 5,
                    "compactedChunks": 3
                },
                {
                    "kind": "gap",
                    "sourceCursor": "provider-cursor",
                    "sequence": 6,
                    "observedAtMs": 1_002,
                    "stream": null,
                    "data": null,
                    "gapReason": "provider_cursor_lost",
                    "fromSequence": null,
                    "throughSequence": null,
                    "compactedChunks": null
                }
            ]))
        );
        assert_eq!(
            chunk_gap_reason_name(BuildLogChunkGapReason::Missing),
            "missing"
        );
        assert_eq!(
            chunk_gap_reason_name(BuildLogChunkGapReason::Retained),
            "retained"
        );
        assert_eq!(
            source_gap_reason_name(BuildLogSourceGapReason::SourceDisconnected),
            "provider_disconnected"
        );
    }
}
