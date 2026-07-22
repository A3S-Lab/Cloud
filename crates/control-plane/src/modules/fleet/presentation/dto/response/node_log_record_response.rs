use crate::modules::fleet::application::{NodeLogGapReason, NodeLogRecord};
use a3s_runtime::contract::{RuntimeLogDiscontinuityReason, RuntimeLogStream};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeLogRecordKind {
    Data,
    Gap,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeLogRecordResponse {
    pub kind: NodeLogRecordKind,
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

impl From<NodeLogRecord> for NodeLogRecordResponse {
    fn from(record: NodeLogRecord) -> Self {
        match record {
            NodeLogRecord::Data(chunk) => Self {
                kind: NodeLogRecordKind::Data,
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
            NodeLogRecord::Gap { metadata, reason } => Self {
                kind: NodeLogRecordKind::Gap,
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
            NodeLogRecord::CompactedGap { range } => Self {
                kind: NodeLogRecordKind::Gap,
                source_cursor: None,
                sequence: range.through_sequence,
                observed_at_ms: None,
                stream: None,
                data: None,
                gap_reason: Some(gap_reason_name(NodeLogGapReason::Compacted)),
                from_sequence: Some(range.first_sequence),
                through_sequence: Some(range.through_sequence),
                compacted_chunks: Some(range.compacted_chunks()),
            },
            NodeLogRecord::ProviderGap { metadata } => Self {
                kind: NodeLogRecordKind::Gap,
                source_cursor: metadata.cursor,
                sequence: metadata.sequence,
                observed_at_ms: Some(metadata.observed_at_ms),
                stream: None,
                data: None,
                gap_reason: Some(provider_gap_reason_name(metadata.reason)),
                from_sequence: None,
                through_sequence: None,
                compacted_chunks: None,
            },
        }
    }
}

const fn gap_reason_name(reason: NodeLogGapReason) -> &'static str {
    match reason {
        NodeLogGapReason::Missing => "missing",
        NodeLogGapReason::Corrupt => "corrupt",
        NodeLogGapReason::Retained => "retained",
        NodeLogGapReason::Compacted => "compacted",
        NodeLogGapReason::ProviderCursorLost => "provider_cursor_lost",
        NodeLogGapReason::ProviderDisconnected => "provider_disconnected",
    }
}

const fn provider_gap_reason_name(reason: RuntimeLogDiscontinuityReason) -> &'static str {
    match reason {
        RuntimeLogDiscontinuityReason::CursorLost => {
            gap_reason_name(NodeLogGapReason::ProviderCursorLost)
        }
        RuntimeLogDiscontinuityReason::SourceDisconnected => {
            gap_reason_name(NodeLogGapReason::ProviderDisconnected)
        }
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
    use crate::modules::fleet::domain::repositories::{NodeLogCompactionRange, NodeLogGapMetadata};
    use crate::modules::shared_kernel::domain::NodeId;
    use chrono::Utc;
    use serde_json::json;

    #[test]
    fn compacted_gap_json_has_explicit_range_and_nullable_source_fields() {
        let response = NodeLogRecordResponse::from(NodeLogRecord::CompactedGap {
            range: NodeLogCompactionRange {
                node_id: NodeId::new(),
                unit_id: "service".into(),
                generation: 1,
                first_sequence: 4,
                through_sequence: 7,
                compacted_at: Utc::now(),
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
        let node_id = NodeId::new();
        let cursor_lost = NodeLogRecordResponse::from(NodeLogRecord::ProviderGap {
            metadata: NodeLogGapMetadata {
                node_id,
                unit_id: "service".into(),
                generation: 1,
                cursor: Some("provider-cursor".into()),
                sequence: 8,
                observed_at_ms: 1_000,
                reason: RuntimeLogDiscontinuityReason::CursorLost,
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

        let disconnected = NodeLogRecordResponse::from(NodeLogRecord::ProviderGap {
            metadata: NodeLogGapMetadata {
                node_id,
                unit_id: "service".into(),
                generation: 1,
                cursor: None,
                sequence: 9,
                observed_at_ms: 1_001,
                reason: RuntimeLogDiscontinuityReason::SourceDisconnected,
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
}
