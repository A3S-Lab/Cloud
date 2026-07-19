use crate::modules::workloads::application::{
    WorkloadLogGapReason, WorkloadLogPage, WorkloadLogRecord,
};
use a3s_runtime::contract::RuntimeLogStream;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadLogsResponse {
    pub workload_id: Uuid,
    pub revision_id: Uuid,
    pub node_id: Option<Uuid>,
    pub unit_id: String,
    pub generation: u64,
    pub records: Vec<WorkloadLogRecordResponse>,
    pub next_cursor: Option<String>,
}

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
    pub source_cursor: String,
    pub sequence: u64,
    pub observed_at_ms: u64,
    pub stream: &'static str,
    pub data: Option<String>,
    pub gap_reason: Option<&'static str>,
}

impl From<WorkloadLogPage> for WorkloadLogsResponse {
    fn from(page: WorkloadLogPage) -> Self {
        Self {
            workload_id: page.workload_id.as_uuid(),
            revision_id: page.revision_id.as_uuid(),
            node_id: page.node_id.map(|node_id| node_id.as_uuid()),
            unit_id: page.unit_id,
            generation: page.generation,
            records: page.records.into_iter().map(Into::into).collect(),
            next_cursor: page
                .next_after_sequence
                .map(|sequence| format!("v1:{sequence}")),
        }
    }
}

impl From<WorkloadLogRecord> for WorkloadLogRecordResponse {
    fn from(record: WorkloadLogRecord) -> Self {
        match record {
            WorkloadLogRecord::Data(chunk) => Self {
                kind: WorkloadLogRecordKind::Data,
                source_cursor: chunk.cursor,
                sequence: chunk.sequence,
                observed_at_ms: chunk.observed_at_ms,
                stream: stream_name(chunk.stream),
                data: Some(chunk.data),
                gap_reason: None,
            },
            WorkloadLogRecord::Gap { metadata, reason } => Self {
                kind: WorkloadLogRecordKind::Gap,
                source_cursor: metadata.cursor,
                sequence: metadata.sequence,
                observed_at_ms: metadata.observed_at_ms,
                stream: stream_name(metadata.stream),
                data: None,
                gap_reason: Some(match reason {
                    WorkloadLogGapReason::Missing => "missing",
                    WorkloadLogGapReason::Corrupt => "corrupt",
                    WorkloadLogGapReason::Retained => "retained",
                }),
            },
        }
    }
}

const fn stream_name(stream: RuntimeLogStream) -> &'static str {
    match stream {
        RuntimeLogStream::Stdout => "stdout",
        RuntimeLogStream::Stderr => "stderr",
    }
}
