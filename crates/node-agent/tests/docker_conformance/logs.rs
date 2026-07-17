use super::fixture::{found, require, resource_id, DockerConformanceFixture};
use super::specs;
use a3s_runtime::contract::{RuntimeLogChunk, RuntimeLogQuery, RuntimeLogStream, RuntimeUnitState};
use a3s_runtime::{RuntimeClient, RuntimeError, RuntimeResult};
use bollard::container::RemoveContainerOptions;

impl DockerConformanceFixture {
    pub(crate) async fn run_logs(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        self.verify_order_filter_cursor_limit_and_retention(client)
            .await?;
        self.verify_large_record(client).await?;
        self.verify_rotation_gap(client).await
    }

    async fn verify_order_filter_cursor_limit_and_retention(
        &self,
        client: &dyn RuntimeClient,
    ) -> RuntimeResult<()> {
        let task = specs::task_spec(
            specs::unit_id(&self.namespace, "logs-complete"),
            "printf 'same-a\\nsame-b\\n'; printf 'error-a\\n' >&2; sleep 1; printf 'later-a\\n'",
        );
        let observation = client
            .apply(&specs::apply("logs-complete-apply", task.clone()))
            .await?;
        require(
            observation.state == RuntimeUnitState::Succeeded,
            "Docker log fixture Task did not finish successfully",
        )?;

        let all = client.logs(&log_query(&task, None, 32, None)).await?;
        for chunk in &all {
            let prefix = chunk.data.chars().take(32).collect::<String>();
            eprintln!(
                "A3S_RUNTIME_LOG_RECORD stream={:?} len={} prefix={prefix:?}",
                chunk.stream,
                chunk.data.len()
            );
        }
        require(
            all.len() >= 4,
            format!("Docker log fixture returned too few records: {}", all.len()),
        )?;
        require_strict_order(&all)?;
        require(
            all.iter().any(|chunk| chunk.data == "same-a\n")
                && all.iter().any(|chunk| chunk.data == "same-b\n")
                && all.iter().any(|chunk| chunk.data == "error-a\n"),
            "Docker logs lost stdout or stderr records",
        )?;
        let stdout = client
            .logs(&log_query(&task, None, 32, Some(RuntimeLogStream::Stdout)))
            .await?;
        let stderr = client
            .logs(&log_query(&task, None, 32, Some(RuntimeLogStream::Stderr)))
            .await?;
        require(
            !stdout.is_empty()
                && stdout
                    .iter()
                    .all(|chunk| chunk.stream == RuntimeLogStream::Stdout)
                && stderr.len() == 1
                && stderr[0].stream == RuntimeLogStream::Stderr
                && stderr[0].data == "error-a\n",
            "Docker log stream filtering mixed or omitted streams",
        )?;

        let first_page = client.logs(&log_query(&task, None, 2, None)).await?;
        require(
            first_page.len() == 2,
            "Docker log limit did not return exactly two available records",
        )?;
        let cursor = first_page
            .last()
            .map(|chunk| chunk.cursor.clone())
            .ok_or_else(|| RuntimeError::Protocol("Docker log first page is empty".into()))?;
        let resumed = client
            .logs(&log_query(&task, Some(cursor.clone()), 32, None))
            .await?;
        require(
            !resumed.is_empty()
                && resumed.iter().all(|chunk| chunk.cursor != cursor)
                && resumed[0].sequence > first_page[1].sequence,
            "Docker log cursor resume duplicated or reordered records",
        )?;

        require(
            all.iter().enumerate().all(|(index, left)| {
                all.iter()
                    .skip(index + 1)
                    .all(|right| left.cursor != right.cursor)
            }),
            "Docker log fixture produced duplicate cursors",
        )?;

        let retained = client.logs(&log_query(&task, None, 1, None)).await?;
        require(
            retained.len() == 1,
            "terminal Docker Task logs were not retained",
        )?;
        client
            .remove(&specs::action("logs-complete-remove", &task))
            .await?;
        require(
            matches!(
                client.logs(&log_query(&task, None, 1, None)).await,
                Err(RuntimeError::NotFound { unit_id }) if unit_id == task.unit_id
            ),
            "removed Runtime unit still exposed Docker logs",
        )
    }

    async fn verify_large_record(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let task = specs::task_spec(
            specs::unit_id(&self.namespace, "logs-large-record"),
            "head -c 1048575 /dev/zero | tr '\\000' x; printf '\\n'",
        );
        let observation = client
            .apply(&specs::apply("logs-large-record-apply", task.clone()))
            .await?;
        require(
            observation.state == RuntimeUnitState::Succeeded,
            "Docker large-log Task did not finish successfully",
        )?;
        let chunks = client
            .logs(&log_query(
                &task,
                None,
                10_000,
                Some(RuntimeLogStream::Stdout),
            ))
            .await?;
        require_strict_order(&chunks)?;
        let total_bytes = chunks.iter().map(|chunk| chunk.data.len()).sum::<usize>();
        require(
            chunks.len() > 1
                && total_bytes == 1024 * 1024
                && chunks.iter().all(|chunk| chunk.data.len() <= 1024 * 1024),
            format!(
                "Docker large log was not losslessly bounded: chunks={}, bytes={total_bytes}",
                chunks.len()
            ),
        )?;
        client
            .remove(&specs::action("logs-large-record-remove", &task))
            .await?;
        Ok(())
    }

    async fn verify_rotation_gap(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let service = specs::service_spec(
            specs::unit_id(&self.namespace, "logs-gap"),
            "printf 'before-gap\\n'; exec sleep 300",
        );
        let first = client
            .apply(&specs::apply("logs-gap-initial", service.clone()))
            .await?;
        let mut chunks = Vec::new();
        for _ in 0..20 {
            chunks = client.logs(&log_query(&service, None, 10, None)).await?;
            if !chunks.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let old_cursor = chunks
            .first()
            .map(|chunk| chunk.cursor.clone())
            .ok_or_else(|| RuntimeError::Protocol("Docker gap fixture emitted no log".into()))?;
        self.docker_call(
            "replace log source externally",
            self.docker.remove_container(
                resource_id(&first)?,
                Some(RemoveContainerOptions {
                    force: true,
                    v: false,
                    link: false,
                }),
            ),
        )
        .await?;
        let lost = found(client.inspect(&service.unit_id).await?)?;
        require(
            lost.state == RuntimeUnitState::Unknown,
            "external log source deletion did not become unknown",
        )?;
        let replacement = client
            .apply(&specs::apply("logs-gap-replacement", service.clone()))
            .await?;
        require(
            replacement.state == RuntimeUnitState::Running,
            "replacement log source did not start",
        )?;
        let gap = client
            .logs(&log_query(&service, Some(old_cursor), 10, None))
            .await;
        require(
            matches!(
                gap,
                Err(RuntimeError::Protocol(message))
                    if message.contains("explicit gap")
            ),
            "Docker log rotation/replacement did not report an explicit cursor gap",
        )?;
        client
            .remove(&specs::action("logs-gap-remove", &service))
            .await?;
        Ok(())
    }
}

fn log_query(
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
    cursor: Option<String>,
    limit: u32,
    stream: Option<RuntimeLogStream>,
) -> RuntimeLogQuery {
    RuntimeLogQuery {
        schema: RuntimeLogQuery::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        cursor,
        limit,
        stream,
    }
}

fn require_strict_order(chunks: &[RuntimeLogChunk]) -> RuntimeResult<()> {
    require(
        chunks
            .windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence),
        "Docker logs are not in strict total order",
    )
}
