use super::container::container_id;
use super::{docker_error, is_status, DockerRuntimeDriver};
use crate::SecretMaterial;
use a3s_runtime::contract::{
    RuntimeLogChunk, RuntimeLogDiscontinuityReason, RuntimeLogQuery, RuntimeLogStream,
};
use a3s_runtime::{RuntimeError, RuntimeResult, RuntimeUnitRecord};
use bollard::container::{LogOutput, LogsOptions};
use chrono::DateTime;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const REDACTED_SECRET: &str = "[REDACTED]";

impl DockerRuntimeDriver {
    pub(super) async fn read_logs(
        &self,
        unit: &RuntimeUnitRecord,
        query: &RuntimeLogQuery,
    ) -> RuntimeResult<Vec<RuntimeLogChunk>> {
        let node_id = self.bound_node_id().await?;
        let digest = unit.spec.digest().map_err(RuntimeError::Protocol)?;
        let container = self
            .find_container(node_id, &unit.spec, &digest)
            .await?
            .ok_or_else(|| {
                log_discontinuity(
                    unit,
                    query,
                    RuntimeLogDiscontinuityReason::SourceDisconnected,
                )
            })?;
        let redaction_materials = self.resolve_log_redaction_materials(&unit.spec).await?;
        let cursor = query.cursor.as_deref().map(LogCursor::parse).transpose()?;
        let since = cursor
            .as_ref()
            .map_or(0, |cursor| cursor.seconds.saturating_sub(1));
        let options: LogsOptions<String> = LogsOptions {
            follow: false,
            stdout: query.stream != Some(RuntimeLogStream::Stderr),
            stderr: query.stream != Some(RuntimeLogStream::Stdout),
            since,
            until: 0,
            timestamps: true,
            tail: "all".into(),
        };
        let mut stream = self.docker.logs(&container_id(&container)?, Some(options));
        let target_cursor = cursor.as_ref().map(LogCursor::encode);
        let mut cursor_found = target_cursor.is_none();
        let mut chunks = Vec::with_capacity(query.limit as usize);
        let mut per_second = 0_u64;
        let mut previous_second = None;
        let mut previous_sequence = None;
        'logs: while let Some(output) = stream.next().await {
            let output = output.map_err(|error| {
                if is_status(&error, 404) {
                    log_discontinuity(
                        unit,
                        query,
                        RuntimeLogDiscontinuityReason::SourceDisconnected,
                    )
                } else {
                    docker_error(error)
                }
            })?;
            let stream = match output {
                LogOutput::StdErr { .. } => RuntimeLogStream::Stderr,
                LogOutput::StdOut { .. } | LogOutput::Console { .. } => RuntimeLogStream::Stdout,
                LogOutput::StdIn { .. } => continue,
            };
            let text = Zeroizing::new(output.to_string());
            for line in text.split_inclusive('\n') {
                let (timestamp, data) = parse_docker_log_line(line)?;
                if data.len() > 1024 * 1024 {
                    return Err(RuntimeError::Protocol(
                        "Docker log record exceeds the Runtime one-MiB chunk bound".into(),
                    ));
                }
                let record = RawLogRecord {
                    timestamp_ns: timestamp,
                    stream,
                    data: redact_log_data(data, &redaction_materials),
                };
                let seconds = record.timestamp_ns.div_euclid(1_000_000_000);
                if previous_second != Some(seconds) {
                    previous_second = Some(seconds);
                    per_second = 0;
                }
                if per_second >= 1_000_000 {
                    return Err(RuntimeError::Protocol(
                        "Docker emitted more than one million log records in one second".into(),
                    ));
                }
                let record_cursor = LogCursor::new(&record, per_second)?;
                per_second += 1;
                let sequence = log_sequence(record.timestamp_ns, per_second)?;
                if previous_sequence.is_some_and(|previous| previous >= sequence) {
                    return Err(RuntimeError::Protocol(
                        "Docker log records are not strictly ordered".into(),
                    ));
                }
                previous_sequence = Some(sequence);
                let chunk = RuntimeLogChunk {
                    schema: RuntimeLogChunk::SCHEMA.into(),
                    cursor: record_cursor.encode(),
                    sequence,
                    observed_at_ms: u64::try_from(record.timestamp_ns.div_euclid(1_000_000))
                        .map_err(|_| {
                            RuntimeError::Protocol("Docker log timestamp is invalid".into())
                        })?,
                    stream: record.stream,
                    data: record.data,
                };
                chunk.validate().map_err(RuntimeError::Protocol)?;
                if !cursor_found {
                    if target_cursor.as_deref() == Some(chunk.cursor.as_str()) {
                        cursor_found = true;
                    }
                    continue;
                }
                chunks.push(chunk);
                if chunks.len() == query.limit as usize {
                    break 'logs;
                }
            }
        }
        if !cursor_found {
            return Err(log_discontinuity(
                unit,
                query,
                RuntimeLogDiscontinuityReason::CursorLost,
            ));
        }
        Ok(chunks)
    }
}

fn log_discontinuity(
    unit: &RuntimeUnitRecord,
    query: &RuntimeLogQuery,
    reason: RuntimeLogDiscontinuityReason,
) -> RuntimeError {
    RuntimeError::LogDiscontinuity {
        unit_id: unit.spec.unit_id.clone(),
        generation: unit.spec.generation,
        cursor: query.cursor.clone(),
        reason,
    }
}

struct RawLogRecord {
    timestamp_ns: i64,
    stream: RuntimeLogStream,
    data: String,
}

struct LogCursor {
    seconds: i64,
    timestamp_ns: i64,
    ordinal: u64,
    stream: RuntimeLogStream,
    digest: String,
}

impl LogCursor {
    fn new(record: &RawLogRecord, ordinal: u64) -> RuntimeResult<Self> {
        let mut hash = Sha256::new();
        hash.update(match record.stream {
            RuntimeLogStream::Stdout => b"stdout".as_slice(),
            RuntimeLogStream::Stderr => b"stderr".as_slice(),
        });
        hash.update(record.timestamp_ns.to_be_bytes());
        hash.update(record.data.as_bytes());
        let digest = format!("{:x}", hash.finalize());
        Ok(Self {
            seconds: record.timestamp_ns.div_euclid(1_000_000_000),
            timestamp_ns: record.timestamp_ns,
            ordinal,
            stream: record.stream,
            digest: digest[..16].into(),
        })
    }

    fn parse(value: &str) -> RuntimeResult<Self> {
        let fields = value.split(':').collect::<Vec<_>>();
        if fields.len() != 6 || fields[0] != "v1" {
            return Err(RuntimeError::InvalidRequest(
                "invalid Docker log cursor".into(),
            ));
        }
        let seconds = fields[1]
            .parse::<i64>()
            .map_err(|_| RuntimeError::InvalidRequest("invalid Docker log cursor".into()))?;
        let timestamp_ns = fields[2]
            .parse::<i64>()
            .map_err(|_| RuntimeError::InvalidRequest("invalid Docker log cursor".into()))?;
        let ordinal = fields[3]
            .parse::<u64>()
            .map_err(|_| RuntimeError::InvalidRequest("invalid Docker log cursor".into()))?;
        let stream = match fields[4] {
            "o" => RuntimeLogStream::Stdout,
            "e" => RuntimeLogStream::Stderr,
            _ => {
                return Err(RuntimeError::InvalidRequest(
                    "invalid Docker log cursor".into(),
                ))
            }
        };
        let digest = fields[5];
        if seconds != timestamp_ns.div_euclid(1_000_000_000)
            || digest.len() != 16
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(RuntimeError::InvalidRequest(
                "invalid Docker log cursor".into(),
            ));
        }
        Ok(Self {
            seconds,
            timestamp_ns,
            ordinal,
            stream,
            digest: digest.into(),
        })
    }

    fn encode(&self) -> String {
        let stream = match self.stream {
            RuntimeLogStream::Stdout => "o",
            RuntimeLogStream::Stderr => "e",
        };
        format!(
            "v1:{}:{}:{}:{stream}:{}",
            self.seconds, self.timestamp_ns, self.ordinal, self.digest
        )
    }
}

fn parse_docker_log_line(value: &str) -> RuntimeResult<(i64, &str)> {
    let (timestamp, data) = value.split_once(' ').ok_or_else(|| {
        RuntimeError::Protocol("Docker timestamped log record has no timestamp".into())
    })?;
    let timestamp = DateTime::parse_from_rfc3339(timestamp)
        .map_err(|_| RuntimeError::Protocol("Docker log timestamp is invalid".into()))?
        .timestamp_nanos_opt()
        .ok_or_else(|| RuntimeError::Protocol("Docker log timestamp is out of range".into()))?;
    Ok((timestamp, data))
}

fn log_sequence(timestamp_ns: i64, ordinal_after_increment: u64) -> RuntimeResult<u64> {
    u64::try_from(timestamp_ns.div_euclid(1_000_000_000))
        .ok()
        .and_then(|seconds| seconds.checked_mul(1_000_000))
        .and_then(|base| base.checked_add(ordinal_after_increment))
        .ok_or_else(|| RuntimeError::Protocol("Docker log sequence overflowed".into()))
}

fn redact_log_data(data: &str, materials: &[SecretMaterial]) -> String {
    let mut patterns = materials
        .iter()
        .filter_map(|material| std::str::from_utf8(material.as_bytes()).ok())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    patterns.sort_unstable_by_key(|value| std::cmp::Reverse(value.len()));
    patterns.dedup();
    if patterns.is_empty() {
        return data.into();
    }
    let mut redacted = String::with_capacity(data.len());
    let mut offset = 0_usize;
    while offset < data.len() {
        let remaining = &data[offset..];
        if let Some(pattern) = patterns
            .iter()
            .find(|pattern| remaining.starts_with(**pattern))
        {
            redacted.push_str(REDACTED_SECRET);
            offset += pattern.len();
            continue;
        }
        let Some(character) = remaining.chars().next() else {
            break;
        };
        redacted.push(character);
        offset += character.len_utf8();
    }
    redacted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trip_is_stable() {
        let record = RawLogRecord {
            timestamp_ns: 1_700_000_000_123_456_789,
            stream: RuntimeLogStream::Stderr,
            data: "failure\n".into(),
        };
        let cursor = LogCursor::new(&record, 7).expect("cursor");
        let encoded = cursor.encode();
        assert_eq!(LogCursor::parse(&encoded).expect("parse").encode(), encoded);
    }

    #[test]
    fn cursor_ordinal_distinguishes_records_with_one_provider_timestamp() {
        let first = RawLogRecord {
            timestamp_ns: 1_700_000_000_123_456_789,
            stream: RuntimeLogStream::Stdout,
            data: "first\n".into(),
        };
        let second = RawLogRecord {
            timestamp_ns: first.timestamp_ns,
            stream: RuntimeLogStream::Stdout,
            data: "second\n".into(),
        };
        let first = LogCursor::new(&first, 0).expect("first cursor");
        let second = LogCursor::new(&second, 1).expect("second cursor");
        assert_eq!(first.timestamp_ns, second.timestamp_ns);
        assert_ne!(first.encode(), second.encode());
        assert_eq!(
            LogCursor::parse(&second.encode()).expect("parse").ordinal,
            1
        );
        assert!(
            log_sequence(first.timestamp_ns, 1).expect("first sequence")
                < log_sequence(second.timestamp_ns, 2).expect("second sequence")
        );
    }

    #[test]
    fn exact_secret_values_are_redacted_without_overlap_or_marker_reprocessing() {
        let materials = vec![
            SecretMaterial::new(b"token".to_vec()).expect("short Secret"),
            SecretMaterial::new(b"token-value".to_vec()).expect("long Secret"),
            SecretMaterial::new(b"REDACTED".to_vec()).expect("marker Secret"),
        ];
        let redacted = redact_log_data("token-value token REDACTED remains private\n", &materials);
        assert_eq!(
            redacted,
            "[REDACTED] [REDACTED] [REDACTED] remains private\n"
        );
        assert!(!redacted.contains("token"));
    }
}
