use super::{polling_sse_stream, PollingSseInitial, PollingSseOptions};
use crate::modules::shared_kernel::application::ApplicationError;
use a3s_boot::{BootError, BootRequest, Result, SseEvent, SseStream};
use serde::Serialize;
use std::future::Future;
use std::time::Duration;

const SEQUENCE_CURSOR_PREFIX: &str = "v1:";
const LIVE_SEQUENCE_POLL_INTERVAL: Duration = Duration::from_secs(1);
const LIVE_SEQUENCE_KEEPALIVE_POLLS: u64 = 15;
const MAX_LIVE_SEQUENCE_EVENT_BYTES: usize = 8 * 1024 * 1024;

pub(crate) const MAX_LIVE_SEQUENCE_RECORDS: u16 = 16;

pub(crate) trait SequenceRecord: Serialize + Send + 'static {
    fn sequence(&self) -> u64;
}

pub(crate) trait SequencePage: Serialize + Send + 'static {
    type Record: SequenceRecord;

    fn records(&self) -> &[Self::Record];

    fn take_records(&mut self) -> Vec<Self::Record>;

    fn replace_records(&mut self, records: Vec<Self::Record>);

    fn set_next_cursor(&mut self, cursor: Option<String>);
}

pub(crate) const fn default_live_sequence_limit() -> u16 {
    MAX_LIVE_SEQUENCE_RECORDS
}

pub(crate) fn parse_sequence_cursor(cursor: &str) -> Option<u64> {
    cursor
        .strip_prefix(SEQUENCE_CURSOR_PREFIX)
        .filter(|sequence| !sequence.is_empty())
        .and_then(|sequence| sequence.parse::<u64>().ok())
}

pub(crate) fn format_sequence_cursor(sequence: u64) -> String {
    format!("{SEQUENCE_CURSOR_PREFIX}{sequence}")
}

pub(crate) fn decode_sequence_cursor(
    cursor: Option<&str>,
    stream_name: &str,
) -> Result<Option<u64>> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    parse_sequence_cursor(cursor)
        .map(Some)
        .ok_or_else(|| BootError::BadRequest(format!("invalid {stream_name} cursor")))
}

pub(crate) fn resolve_sequence_cursor(
    request: &BootRequest,
    query_cursor: Option<&str>,
    stream_name: &str,
) -> Result<Option<u64>> {
    let cursor = request
        .header("last-event-id")
        .filter(|event_id| !event_id.is_empty())
        .or(query_cursor);
    decode_sequence_cursor(cursor, stream_name)
}

pub(crate) fn sequence_stream_error(
    error: ApplicationError,
    internal_message: &'static str,
) -> BootError {
    match error {
        ApplicationError::Invalid(message) => BootError::BadRequest(message),
        ApplicationError::NotFound(message) => BootError::NotFound(message),
        ApplicationError::Forbidden(message) => BootError::Forbidden(message),
        ApplicationError::Conflict(_)
        | ApplicationError::Unavailable(_)
        | ApplicationError::Internal(_) => BootError::Internal(internal_message.into()),
    }
}

pub(crate) async fn stream_sequence_pages<Q, P, Load, LoadFuture, Advance>(
    mut query: Q,
    load_page: Load,
    advance: Advance,
    stream_name: &'static str,
) -> Result<SseStream>
where
    Q: Clone + Send + 'static,
    P: SequencePage,
    Load: Fn(Q) -> LoadFuture + Send + Sync + 'static,
    LoadFuture: Future<Output = Result<P>> + Send + 'static,
    Advance: Fn(&mut Q, u64) + Clone + Send + Sync + 'static,
{
    let initial = load_page(query.clone()).await?;
    let initial = sequence_page_event(&mut query, initial, &advance, stream_name)?;
    let options = PollingSseOptions::new(
        &format!("live {stream_name}"),
        LIVE_SEQUENCE_POLL_INTERVAL,
        LIVE_SEQUENCE_KEEPALIVE_POLLS,
    )?;

    Ok(polling_sse_stream(
        query,
        PollingSseInitial::Completed(initial),
        move |mut query| {
            let page = load_page(query.clone());
            let advance = advance.clone();
            async move {
                let event = sequence_page_event(&mut query, page.await?, &advance, stream_name)?;
                Ok((query, event))
            }
        },
        options,
    ))
}

struct BoundedSequenceEvent {
    event: SseEvent,
    through_sequence: u64,
}

fn sequence_page_event<Q, P, Advance>(
    query: &mut Q,
    page: P,
    advance: &Advance,
    stream_name: &str,
) -> Result<Option<SseEvent>>
where
    P: SequencePage,
    Advance: Fn(&mut Q, u64),
{
    let Some(event) = bounded_sequence_event(page, stream_name)? else {
        return Ok(None);
    };
    advance(query, event.through_sequence);
    Ok(Some(event.event))
}

fn bounded_sequence_event<P>(
    mut response: P,
    stream_name: &str,
) -> Result<Option<BoundedSequenceEvent>>
where
    P: SequencePage,
{
    let records = response.take_records();
    if records.is_empty() {
        return Ok(None);
    }

    response.set_next_cursor(Some(format_sequence_cursor(u64::MAX)));
    let base_size = serde_json::to_vec(&response)
        .map_err(|error| sequence_serialization_error(stream_name, error))?
        .len();
    let mut records_size = 0_usize;
    let mut record_count = 0_usize;
    for record in &records {
        let encoded_size = serde_json::to_vec(record)
            .map_err(|error| sequence_serialization_error(stream_name, error))?
            .len();
        let separator_size = usize::from(record_count > 0);
        let candidate_size = base_size
            .checked_add(records_size)
            .and_then(|size| size.checked_add(separator_size))
            .and_then(|size| size.checked_add(encoded_size))
            .ok_or_else(|| {
                BootError::Internal(format!("live {stream_name} event size overflowed"))
            })?;
        if candidate_size > MAX_LIVE_SEQUENCE_EVENT_BYTES {
            break;
        }
        records_size = records_size
            .checked_add(separator_size)
            .and_then(|size| size.checked_add(encoded_size))
            .ok_or_else(|| {
                BootError::Internal(format!("live {stream_name} record size overflowed"))
            })?;
        record_count += 1;
    }
    if record_count == 0 {
        return Err(BootError::Internal(format!(
            "one live {stream_name} record exceeds the event bound"
        )));
    }

    response.replace_records(records.into_iter().take(record_count).collect());
    let through_sequence = response
        .records()
        .last()
        .map(SequenceRecord::sequence)
        .ok_or_else(|| {
            BootError::Internal(format!(
                "live {stream_name} event lost its terminal sequence"
            ))
        })?;
    let cursor = format_sequence_cursor(through_sequence);
    response.set_next_cursor(Some(cursor.clone()));
    let encoded = serde_json::to_string(&response)
        .map_err(|error| sequence_serialization_error(stream_name, error))?;
    if encoded.len() > MAX_LIVE_SEQUENCE_EVENT_BYTES {
        return Err(BootError::Internal(format!(
            "live {stream_name} event exceeded its encoded bound"
        )));
    }

    Ok(Some(BoundedSequenceEvent {
        event: SseEvent::new(encoded).with_event("records").with_id(cursor),
        through_sequence,
    }))
}

fn sequence_serialization_error(stream_name: &str, error: serde_json::Error) -> BootError {
    BootError::Internal(format!("live {stream_name} serialization failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_boot::HttpMethod;
    use futures_util::TryStreamExt;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct TestPage {
        records: Vec<TestRecord>,
        next_cursor: Option<String>,
    }

    impl SequencePage for TestPage {
        type Record = TestRecord;

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

    #[derive(Debug, Serialize)]
    struct TestRecord {
        sequence: u64,
        data: String,
    }

    impl SequenceRecord for TestRecord {
        fn sequence(&self) -> u64 {
            self.sequence
        }
    }

    #[derive(Clone)]
    struct TestQuery {
        after_sequence: Option<u64>,
    }

    #[test]
    fn sequence_cursor_round_trip_is_canonical_and_bounded_to_u64() {
        for sequence in [0, 1, u64::MAX] {
            let cursor = format_sequence_cursor(sequence);
            assert_eq!(parse_sequence_cursor(&cursor), Some(sequence));
        }
        for invalid in ["", "v1:", "1", "v2:1", "v1:-1", "v1:18446744073709551616"] {
            assert_eq!(parse_sequence_cursor(invalid), None);
        }
    }

    #[test]
    fn last_event_id_precedes_the_query_cursor_and_empty_headers_fall_back() {
        let request = BootRequest::new(HttpMethod::Get, "/stream?cursor=v1:2")
            .with_header("last-event-id", "v1:7");
        assert_eq!(
            resolve_sequence_cursor(&request, Some("v1:2"), "test stream").expect("header cursor"),
            Some(7)
        );

        let request = BootRequest::new(HttpMethod::Get, "/stream?cursor=v1:2")
            .with_header("last-event-id", "");
        assert_eq!(
            resolve_sequence_cursor(&request, Some("v1:2"), "test stream").expect("query cursor"),
            Some(2)
        );
    }

    #[test]
    fn invalid_cursor_has_a_stream_specific_public_error() {
        let error =
            decode_sequence_cursor(Some("untrusted"), "build log").expect_err("invalid cursor");
        assert!(matches!(
            error,
            BootError::BadRequest(message) if message == "invalid build log cursor"
        ));
    }

    #[test]
    fn sequence_events_are_byte_bounded_and_resume_after_the_last_record() {
        let data = "\0".repeat(1024 * 1024);
        let event = bounded_sequence_event(
            TestPage {
                records: vec![
                    TestRecord {
                        sequence: 1,
                        data: data.clone(),
                    },
                    TestRecord { sequence: 2, data },
                ],
                next_cursor: None,
            },
            "test stream",
        )
        .expect("bounded event")
        .expect("nonempty event");

        assert_eq!(event.through_sequence, 1);
        let encoded = event.event.encode();
        assert!(encoded.len() <= MAX_LIVE_SEQUENCE_EVENT_BYTES + 128);
        let encoded = String::from_utf8(encoded).expect("UTF-8 event");
        assert!(encoded.contains("id: v1:1"));
        assert!(encoded.contains("\"nextCursor\":\"v1:1\""));
        assert!(!encoded.contains("\"sequence\":2"));
    }

    #[tokio::test]
    async fn stream_advances_the_authoritative_query_and_keeps_empty_pages_alive() {
        let observed_cursors = Arc::new(Mutex::new(Vec::new()));
        let loader_cursors = Arc::clone(&observed_cursors);
        let mut stream = stream_sequence_pages(
            TestQuery {
                after_sequence: None,
            },
            move |query: TestQuery| {
                let loader_cursors = Arc::clone(&loader_cursors);
                async move {
                    loader_cursors.lock().await.push(query.after_sequence);
                    Ok(TestPage {
                        records: query
                            .after_sequence
                            .is_none()
                            .then(|| TestRecord {
                                sequence: 4,
                                data: "first".into(),
                            })
                            .into_iter()
                            .collect(),
                        next_cursor: None,
                    })
                }
            },
            |query, sequence| query.after_sequence = Some(sequence),
            "test stream",
        )
        .await
        .expect("sequence stream");

        let records = tokio::time::timeout(Duration::from_millis(100), stream.try_next())
            .await
            .expect("initial event timeout")
            .expect("initial event")
            .expect("records event");
        let encoded = String::from_utf8(records.encode()).expect("UTF-8 records event");
        assert!(encoded.contains("id: v1:4"));

        let keepalive = tokio::time::timeout(Duration::from_millis(1_500), stream.try_next())
            .await
            .expect("keepalive timeout")
            .expect("keepalive event")
            .expect("keepalive");
        let encoded = String::from_utf8(keepalive.encode()).expect("UTF-8 keepalive");
        assert!(encoded.contains(": keepalive"));
        assert_eq!(*observed_cursors.lock().await, vec![None, Some(4)]);
    }

    #[test]
    fn log_controllers_do_not_reimplement_sequence_transport_or_cursor_codecs() {
        for (name, source) in [
            (
                "workload queries",
                include_str!(
                    "../modules/workloads/presentation/controllers/workload_queries_controller.rs"
                ),
            ),
            (
                "build run queries",
                include_str!(
                    "../modules/artifacts/presentation/controllers/build_run_queries_controller.rs"
                ),
            ),
        ] {
            for forbidden in [
                "async_stream::try_stream!",
                "fn decode_cursor(",
                "parse_sequence_cursor",
                "format_sequence_cursor",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "{name} must reuse the shared sequence transport; found {forbidden}"
                );
            }
        }
    }
}
