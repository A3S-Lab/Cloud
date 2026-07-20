use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::workloads::application::{GetWorkloadLogs, WorkloadLogPage};
use crate::modules::workloads::presentation::dto::WorkloadLogsResponse;
use a3s_boot::{BootError, QueryBus, Result, SseEvent, SseStream};
use std::sync::Arc;
use std::time::Duration;

const LIVE_LOG_POLL_INTERVAL: Duration = Duration::from_secs(1);
const LIVE_LOG_KEEPALIVE_POLLS: u64 = 15;
const MAX_LIVE_LOG_EVENT_BYTES: usize = 8 * 1024 * 1024;

pub(super) async fn workload_log_stream(
    bus: Arc<QueryBus>,
    mut query: GetWorkloadLogs,
) -> Result<SseStream> {
    let initial = load_page(&bus, query.clone()).await?;
    Ok(Box::pin(async_stream::try_stream! {
        let mut initial = Some(initial);
        let mut empty_polls = 0_u64;
        let mut interval = tokio::time::interval(LIVE_LOG_POLL_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.tick().await;
        loop {
            let page = match initial.take() {
                Some(page) => page,
                None => {
                    interval.tick().await;
                    load_page(&bus, query.clone()).await?
                }
            };
            if let Some(event) = bounded_log_event(page)? {
                query.after_sequence = Some(event.through_sequence);
                empty_polls = 0;
                yield event.event;
                continue;
            }
            empty_polls = empty_polls.saturating_add(1);
            if empty_polls == 1 || empty_polls % LIVE_LOG_KEEPALIVE_POLLS == 0 {
                yield SseEvent::comment("keepalive").with_retry(
                    u64::try_from(LIVE_LOG_POLL_INTERVAL.as_millis())
                        .map_err(|_| BootError::Internal(
                            "live log retry duration overflowed".into()
                        ))?
                );
            }
        }
    }))
}

async fn load_page(bus: &QueryBus, query: GetWorkloadLogs) -> Result<WorkloadLogPage> {
    bus.execute(query).await?.map_err(stream_error)
}

fn stream_error(error: ApplicationError) -> BootError {
    match error {
        ApplicationError::Invalid(message) => BootError::BadRequest(message),
        ApplicationError::NotFound(message) => BootError::NotFound(message),
        ApplicationError::Forbidden(message) => BootError::Forbidden(message),
        ApplicationError::Conflict(_) | ApplicationError::Internal(_) => {
            BootError::Internal("live workload log query failed".into())
        }
    }
}

struct BoundedLogEvent {
    event: SseEvent,
    through_sequence: u64,
}

fn bounded_log_event(page: WorkloadLogPage) -> Result<Option<BoundedLogEvent>> {
    let mut response = WorkloadLogsResponse::from(page);
    if response.records.is_empty() {
        return Ok(None);
    }
    let records = std::mem::take(&mut response.records);
    response.next_cursor = Some(format!("v1:{}", u64::MAX));
    let base_size = serde_json::to_vec(&response)
        .map_err(|error| BootError::Internal(error.to_string()))?
        .len();
    let mut records_size = 0_usize;
    let mut record_count = 0_usize;
    for record in &records {
        let encoded_size = serde_json::to_vec(record)
            .map_err(|error| BootError::Internal(error.to_string()))?
            .len();
        let separator_size = usize::from(record_count > 0);
        let candidate_size = base_size
            .checked_add(records_size)
            .and_then(|size| size.checked_add(separator_size))
            .and_then(|size| size.checked_add(encoded_size))
            .ok_or_else(|| BootError::Internal("live log event size overflowed".into()))?;
        if candidate_size > MAX_LIVE_LOG_EVENT_BYTES {
            break;
        }
        records_size = records_size
            .checked_add(separator_size)
            .and_then(|size| size.checked_add(encoded_size))
            .ok_or_else(|| BootError::Internal("live log record size overflowed".into()))?;
        record_count += 1;
    }
    if record_count == 0 {
        return Err(BootError::Internal(
            "one live workload log record exceeds the event bound".into(),
        ));
    }
    response.records = records.into_iter().take(record_count).collect();
    let through_sequence = response
        .records
        .last()
        .map(|record| record.sequence)
        .ok_or_else(|| BootError::Internal("live log event lost its terminal sequence".into()))?;
    let cursor = format!("v1:{through_sequence}");
    response.next_cursor = Some(cursor.clone());
    let encoded =
        serde_json::to_string(&response).map_err(|error| BootError::Internal(error.to_string()))?;
    if encoded.len() > MAX_LIVE_LOG_EVENT_BYTES {
        return Err(BootError::Internal(
            "live workload log event exceeded its encoded bound".into(),
        ));
    }
    Ok(Some(BoundedLogEvent {
        event: SseEvent::new(encoded)
            .with_event("records")
            .with_id(cursor)
            .with_retry(
                u64::try_from(LIVE_LOG_POLL_INTERVAL.as_millis()).map_err(|_| {
                    BootError::Internal("live log retry duration overflowed".into())
                })?,
            ),
        through_sequence,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{WorkloadId, WorkloadRevisionId};
    use crate::modules::workloads::application::WorkloadLogRecord;
    use a3s_runtime::contract::{RuntimeLogChunk, RuntimeLogStream};

    #[test]
    fn live_log_events_are_byte_bounded_and_resume_after_the_last_included_record() {
        let data = "\0".repeat(1024 * 1024);
        let event = bounded_log_event(WorkloadLogPage {
            workload_id: WorkloadId::new(),
            revision_id: WorkloadRevisionId::new(),
            node_id: None,
            unit_id: "service".into(),
            generation: 1,
            records: vec![
                WorkloadLogRecord::Data(chunk(1, data.clone())),
                WorkloadLogRecord::Data(chunk(2, data)),
            ],
            next_after_sequence: None,
        })
        .expect("bounded event")
        .expect("nonempty event");

        assert_eq!(event.through_sequence, 1);
        let encoded = event.event.encode();
        assert!(encoded.len() <= MAX_LIVE_LOG_EVENT_BYTES + 128);
        let encoded = String::from_utf8(encoded).expect("UTF-8 event");
        assert!(encoded.contains("id: v1:1"));
        assert!(encoded.contains("\"nextCursor\":\"v1:1\""));
        assert!(!encoded.contains("\"sequence\":2"));
    }

    fn chunk(sequence: u64, data: String) -> RuntimeLogChunk {
        RuntimeLogChunk {
            schema: RuntimeLogChunk::SCHEMA.into(),
            cursor: format!("cursor:{sequence}"),
            sequence,
            observed_at_ms: sequence,
            stream: RuntimeLogStream::Stdout,
            data,
        }
    }
}
