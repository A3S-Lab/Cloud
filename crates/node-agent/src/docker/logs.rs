use super::container::container_id;
use super::{docker_error, DockerRuntimeDriver};
use a3s_runtime::contract::{RuntimeLogChunk, RuntimeLogQuery, RuntimeLogStream};
use a3s_runtime::{RuntimeError, RuntimeResult, RuntimeUnitRecord};
use bollard::container::{LogOutput, LogsOptions};
use chrono::DateTime;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};

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
            .ok_or_else(|| RuntimeError::NotFound {
                unit_id: unit.spec.unit_id.clone(),
            })?;
        let cursor = query.cursor.as_deref().map(LogCursor::parse).transpose()?;
        let since = cursor
            .as_ref()
            .map_or(0, |cursor| cursor.seconds.saturating_sub(1));
        let options = LogsOptions {
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
            let output = output.map_err(docker_error)?;
            let stream = match output {
                LogOutput::StdErr { .. } => RuntimeLogStream::Stderr,
                LogOutput::StdOut { .. } | LogOutput::Console { .. } => RuntimeLogStream::Stdout,
                LogOutput::StdIn { .. } => continue,
            };
            let text = output.to_string();
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
                    data: data.into(),
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
                let sequence = u64::try_from(seconds)
                    .ok()
                    .and_then(|seconds| seconds.checked_mul(1_000_000))
                    .and_then(|base| base.checked_add(per_second))
                    .ok_or_else(|| {
                        RuntimeError::Protocol("Docker log sequence overflowed".into())
                    })?;
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
            return Err(RuntimeError::Protocol(
                "Docker log cursor is no longer available; the stream contains an explicit gap"
                    .into(),
            ));
        }
        Ok(chunks)
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
    }
}
