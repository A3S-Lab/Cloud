use a3s_boot::{BootError, Result, SseEvent, SseStream};
use std::future::Future;
use std::time::Duration;

pub(crate) enum PollingSseInitial {
    Completed(Option<SseEvent>),
    Deferred,
}

#[derive(Clone, Copy)]
pub(crate) struct PollingSseOptions {
    poll_interval: Duration,
    keepalive_polls: u64,
    retry_ms: u64,
}

impl PollingSseOptions {
    pub(crate) fn new(
        stream_name: &str,
        poll_interval: Duration,
        keepalive_polls: u64,
    ) -> Result<Self> {
        let retry_ms = u64::try_from(poll_interval.as_millis())
            .map_err(|_| BootError::Internal(format!("{stream_name} retry duration overflowed")))?;
        if retry_ms == 0 || keepalive_polls == 0 {
            return Err(BootError::Internal(format!(
                "{stream_name} polling policy must have a positive millisecond interval and keepalive cadence"
            )));
        }
        Ok(Self {
            poll_interval,
            keepalive_polls,
            retry_ms,
        })
    }
}

pub(crate) fn polling_sse_stream<State, Poll, PollFuture>(
    mut state: State,
    initial: PollingSseInitial,
    poll: Poll,
    options: PollingSseOptions,
) -> SseStream
where
    State: Send + 'static,
    Poll: Fn(State) -> PollFuture + Send + Sync + 'static,
    PollFuture: Future<Output = Result<(State, Option<SseEvent>)>> + Send + 'static,
{
    Box::pin(async_stream::try_stream! {
        let mut initial = Some(initial);
        let mut empty_polls = 0_u64;
        let mut interval = tokio::time::interval(options.poll_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.tick().await;
        loop {
            let event = match initial.take() {
                Some(PollingSseInitial::Completed(event)) => event,
                Some(PollingSseInitial::Deferred) => {
                    let (next_state, event) = poll(state).await?;
                    state = next_state;
                    event
                }
                None => {
                    interval.tick().await;
                    let (next_state, event) = poll(state).await?;
                    state = next_state;
                    event
                }
            };
            if let Some(event) = event {
                empty_polls = 0;
                yield event.with_retry(options.retry_ms);
                continue;
            }
            empty_polls = empty_polls.saturating_add(1);
            if empty_polls == 1 || empty_polls % options.keepalive_polls == 0 {
                yield SseEvent::comment("keepalive").with_retry(options.retry_ms);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polling_policy_requires_bounded_positive_values() {
        assert!(PollingSseOptions::new("test stream", Duration::ZERO, 1).is_err());
        assert!(PollingSseOptions::new("test stream", Duration::from_nanos(1), 1).is_err());
        assert!(PollingSseOptions::new("test stream", Duration::from_millis(1), 0).is_err());
        assert!(PollingSseOptions::new("test stream", Duration::from_millis(1), 1).is_ok());
    }

    #[test]
    fn polling_transport_is_not_reimplemented_by_consumers() {
        for (name, source) in [
            ("sequence stream", include_str!("sequence_stream.rs")),
            (
                "operation queries",
                include_str!(
                    "../modules/operations/presentation/controllers/operations_query_controller.rs"
                ),
            ),
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
            let production = source.split("#[cfg(test)]").next().unwrap_or(source);
            for forbidden in ["async_stream::try_stream!", "tokio::time::interval("] {
                assert!(
                    !production.contains(forbidden),
                    "{name} must reuse the shared polling SSE transport; found {forbidden}"
                );
            }
        }
    }
}
