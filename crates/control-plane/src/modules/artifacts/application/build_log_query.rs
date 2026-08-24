use crate::modules::shared_kernel::domain::{BuildRunId, OperationId, OrganizationId};
use async_trait::async_trait;

pub const MAX_BUILD_LOG_PAGE_SIZE: u16 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildLogStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildLogChunkGapReason {
    Missing,
    Corrupt,
    Retained,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildLogSourceGapReason {
    CursorLost,
    SourceDisconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildLogData {
    source_cursor: String,
    sequence: u64,
    observed_at_ms: u64,
    stream: BuildLogStream,
    data: String,
}

impl BuildLogData {
    pub fn new(
        source_cursor: String,
        sequence: u64,
        observed_at_ms: u64,
        stream: BuildLogStream,
        data: String,
    ) -> Result<Self, String> {
        validate_source_cursor(&source_cursor)?;
        Ok(Self {
            source_cursor,
            sequence,
            observed_at_ms,
            stream,
            data,
        })
    }

    pub fn source_cursor(&self) -> &str {
        &self.source_cursor
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub const fn observed_at_ms(&self) -> u64 {
        self.observed_at_ms
    }

    pub const fn stream(&self) -> BuildLogStream {
        self.stream
    }

    pub fn data(&self) -> &str {
        &self.data
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildLogChunkGap {
    source_cursor: String,
    sequence: u64,
    observed_at_ms: u64,
    stream: BuildLogStream,
    reason: BuildLogChunkGapReason,
}

impl BuildLogChunkGap {
    pub fn new(
        source_cursor: String,
        sequence: u64,
        observed_at_ms: u64,
        stream: BuildLogStream,
        reason: BuildLogChunkGapReason,
    ) -> Result<Self, String> {
        validate_source_cursor(&source_cursor)?;
        Ok(Self {
            source_cursor,
            sequence,
            observed_at_ms,
            stream,
            reason,
        })
    }

    pub fn source_cursor(&self) -> &str {
        &self.source_cursor
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub const fn observed_at_ms(&self) -> u64 {
        self.observed_at_ms
    }

    pub const fn stream(&self) -> BuildLogStream {
        self.stream
    }

    pub const fn reason(&self) -> BuildLogChunkGapReason {
        self.reason
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildLogCompactedRange {
    from_sequence: u64,
    through_sequence: u64,
    compacted_chunks: u64,
}

impl BuildLogCompactedRange {
    pub fn new(from_sequence: u64, through_sequence: u64) -> Result<Self, String> {
        let compacted_chunks = through_sequence
            .checked_sub(from_sequence)
            .and_then(|distance| distance.checked_add(1))
            .ok_or_else(|| "build log compacted range is invalid".to_owned())?;
        Ok(Self {
            from_sequence,
            through_sequence,
            compacted_chunks,
        })
    }

    pub const fn from_sequence(&self) -> u64 {
        self.from_sequence
    }

    pub const fn through_sequence(&self) -> u64 {
        self.through_sequence
    }

    pub const fn compacted_chunks(&self) -> u64 {
        self.compacted_chunks
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildLogSourceGap {
    source_cursor: Option<String>,
    sequence: u64,
    observed_at_ms: u64,
    reason: BuildLogSourceGapReason,
}

impl BuildLogSourceGap {
    pub fn new(
        source_cursor: Option<String>,
        sequence: u64,
        observed_at_ms: u64,
        reason: BuildLogSourceGapReason,
    ) -> Result<Self, String> {
        if let Some(cursor) = source_cursor.as_deref() {
            validate_source_cursor(cursor)?;
        }
        Ok(Self {
            source_cursor,
            sequence,
            observed_at_ms,
            reason,
        })
    }

    pub fn source_cursor(&self) -> Option<&str> {
        self.source_cursor.as_deref()
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub const fn observed_at_ms(&self) -> u64 {
        self.observed_at_ms
    }

    pub const fn reason(&self) -> BuildLogSourceGapReason {
        self.reason
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildLogRecord {
    Data(BuildLogData),
    ChunkGap(BuildLogChunkGap),
    Compacted(BuildLogCompactedRange),
    SourceGap(BuildLogSourceGap),
}

impl BuildLogRecord {
    pub const fn first_sequence(&self) -> u64 {
        match self {
            Self::Data(record) => record.sequence(),
            Self::ChunkGap(record) => record.sequence(),
            Self::Compacted(record) => record.from_sequence(),
            Self::SourceGap(record) => record.sequence(),
        }
    }

    pub const fn through_sequence(&self) -> u64 {
        match self {
            Self::Data(record) => record.sequence(),
            Self::ChunkGap(record) => record.sequence(),
            Self::Compacted(record) => record.through_sequence(),
            Self::SourceGap(record) => record.sequence(),
        }
    }

    const fn stream(&self) -> Option<BuildLogStream> {
        match self {
            Self::Data(record) => Some(record.stream()),
            Self::ChunkGap(record) => Some(record.stream()),
            Self::Compacted(_) | Self::SourceGap(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildLogPage {
    records: Vec<BuildLogRecord>,
    next_after_sequence: Option<u64>,
}

impl BuildLogPage {
    pub fn new(
        records: Vec<BuildLogRecord>,
        next_after_sequence: Option<u64>,
    ) -> Result<Self, String> {
        let mut previous_through = None;
        for record in &records {
            if previous_through.is_some_and(|previous| record.first_sequence() <= previous) {
                return Err(
                    "build log records must be strictly ordered and non-overlapping".into(),
                );
            }
            previous_through = Some(record.through_sequence());
        }
        if next_after_sequence.is_some() && next_after_sequence != previous_through {
            return Err("build log next cursor must equal the final record sequence".into());
        }
        Ok(Self {
            records,
            next_after_sequence,
        })
    }

    pub fn records(&self) -> &[BuildLogRecord] {
        &self.records
    }

    pub const fn next_after_sequence(&self) -> Option<u64> {
        self.next_after_sequence
    }

    pub fn into_parts(self) -> (Vec<BuildLogRecord>, Option<u64>) {
        (self.records, self.next_after_sequence)
    }

    pub fn validate_for(&self, request: &BuildLogReadRequest) -> Result<(), String> {
        if self.records.len() > usize::from(request.limit) {
            return Err("build log page exceeded the requested limit".into());
        }
        if let Some(after_sequence) = request.after_sequence {
            if self
                .records
                .first()
                .is_some_and(|record| record.through_sequence() <= after_sequence)
            {
                return Err("build log page did not advance beyond the requested cursor".into());
            }
        }
        if let Some(stream) = request.stream {
            if self
                .records
                .iter()
                .filter_map(BuildLogRecord::stream)
                .any(|record_stream| record_stream != stream)
            {
                return Err("build log page violated the requested stream filter".into());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildLogReadRequest {
    pub organization_id: OrganizationId,
    pub build_run_id: BuildRunId,
    pub operation_id: OperationId,
    pub attempt: u32,
    pub after_sequence: Option<u64>,
    pub limit: u16,
    pub stream: Option<BuildLogStream>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildLogQueryError {
    #[error("build logs are unavailable: {0}")]
    Unavailable(String),
    #[error("build log query failed: {0}")]
    Internal(String),
}

/// Artifacts-owned query port for the observable output of one BuildRun.
///
/// Implementations may translate a future Box build-log contract, but Fleet
/// node placement, unit identity, persistence records, and response DTOs never
/// cross this boundary.
#[async_trait]
pub trait IBuildLogQueryPort: Send + Sync {
    async fn read(&self, request: BuildLogReadRequest) -> Result<BuildLogPage, BuildLogQueryError>;
}

fn validate_source_cursor(cursor: &str) -> Result<(), String> {
    if cursor.is_empty() || cursor.len() > 4096 || cursor.chars().any(char::is_control) {
        return Err("build log source cursor is invalid".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> BuildLogReadRequest {
        BuildLogReadRequest {
            organization_id: OrganizationId::new(),
            build_run_id: BuildRunId::new(),
            operation_id: OperationId::new(),
            attempt: 1,
            after_sequence: Some(3),
            limit: 2,
            stream: Some(BuildLogStream::Stdout),
        }
    }

    #[test]
    fn page_rejects_overlap_cursor_and_stream_contract_violations() {
        let compacted = BuildLogRecord::Compacted(
            BuildLogCompactedRange::new(4, 6).expect("valid compacted range"),
        );
        let overlapping = BuildLogRecord::Data(
            BuildLogData::new("cursor-6".into(), 6, 10, BuildLogStream::Stdout, "x".into())
                .expect("valid data"),
        );
        assert_eq!(
            BuildLogPage::new(vec![compacted, overlapping], None),
            Err("build log records must be strictly ordered and non-overlapping".into())
        );

        let wrong_cursor = BuildLogPage::new(
            vec![BuildLogRecord::Data(
                BuildLogData::new("cursor-4".into(), 4, 10, BuildLogStream::Stdout, "x".into())
                    .expect("valid data"),
            )],
            Some(5),
        );
        assert_eq!(
            wrong_cursor,
            Err("build log next cursor must equal the final record sequence".into())
        );

        let wrong_stream = BuildLogPage::new(
            vec![BuildLogRecord::Data(
                BuildLogData::new("cursor-4".into(), 4, 10, BuildLogStream::Stderr, "x".into())
                    .expect("valid data"),
            )],
            None,
        )
        .expect("structurally valid page");
        assert_eq!(
            wrong_stream.validate_for(&request()),
            Err("build log page violated the requested stream filter".into())
        );

        let mut overlapping_range_request = request();
        overlapping_range_request.after_sequence = Some(4);
        let overlapping_range = BuildLogPage::new(
            vec![BuildLogRecord::Compacted(
                BuildLogCompactedRange::new(3, 5).expect("valid compacted range"),
            )],
            None,
        )
        .expect("structurally valid page");
        assert_eq!(
            overlapping_range.validate_for(&overlapping_range_request),
            Ok(())
        );

        let stale_range = BuildLogPage::new(
            vec![BuildLogRecord::Compacted(
                BuildLogCompactedRange::new(1, 4).expect("valid compacted range"),
            )],
            None,
        )
        .expect("structurally valid page");
        assert_eq!(
            stale_range.validate_for(&overlapping_range_request),
            Err("build log page did not advance beyond the requested cursor".into())
        );
    }

    #[test]
    fn compacted_range_computes_its_exact_record_count_without_overflow() {
        let range = BuildLogCompactedRange::new(4, 7).expect("valid compacted range");
        assert_eq!(range.compacted_chunks(), 4);
        assert_eq!(
            BuildLogCompactedRange::new(9, 8),
            Err("build log compacted range is invalid".into())
        );
        assert_eq!(
            BuildLogCompactedRange::new(0, u64::MAX),
            Err("build log compacted range is invalid".into())
        );
    }
}
