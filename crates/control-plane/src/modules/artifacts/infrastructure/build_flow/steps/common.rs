use super::super::build_plan::project_build_request;
use super::super::types::BuildFlowInput;
use super::super::{flow_error, BuildFlowRuntime};
use crate::modules::artifacts::domain::{BuildRun, BuildSource};
use a3s_cloud_contracts::{NodeBoxBuildOutput, NodeBoxBuildRequest};
use a3s_flow::FlowError;
use chrono::{DateTime, Utc};

pub(super) async fn load_build(
    runtime: &BuildFlowRuntime,
    run_id: &str,
    input: &BuildFlowInput,
) -> a3s_flow::Result<BuildRun> {
    let build = runtime
        .builds
        .find(input.organization_id, input.build_run_id)
        .await
        .map_err(|error| flow_error("could not load build run", error))?;
    if build.id != input.build_run_id
        || build.organization_id != input.organization_id
        || build.operation_id.to_string() != run_id
        || build.operation_id.as_uuid() != build.id.as_uuid()
    {
        return Err(FlowError::Runtime(
            "build Flow input does not match persisted operation ownership".into(),
        ));
    }
    Ok(build)
}

pub(super) async fn load_source(
    runtime: &BuildFlowRuntime,
    build: &BuildRun,
) -> a3s_flow::Result<BuildSource> {
    let source = runtime
        .sources
        .resolve(build)
        .await
        .map_err(|error| flow_error("could not resolve build source", error))?;
    if source.organization_id != build.organization_id || source.subject != build.subject {
        return Err(FlowError::Runtime(
            "resolved build source does not match persisted build ownership".into(),
        ));
    }
    Ok(source)
}

pub(super) async fn project_request(
    runtime: &BuildFlowRuntime,
    build: &BuildRun,
    source: &BuildSource,
) -> a3s_flow::Result<NodeBoxBuildRequest> {
    let parent_output = load_parent_output(runtime, build).await?;
    project_build_request(&runtime.config, build, source, parent_output.as_ref())
        .map_err(|error| flow_error("could not project Box build request", error))
}

async fn load_parent_output(
    runtime: &BuildFlowRuntime,
    build: &BuildRun,
) -> a3s_flow::Result<Option<NodeBoxBuildOutput>> {
    let Some(parent_id) = build.retry_of_build_run_id else {
        return Ok(None);
    };
    let parent = runtime
        .builds
        .find(build.organization_id, parent_id)
        .await
        .map_err(|error| flow_error("could not load parent build cache", error))?;
    if parent.organization_id != build.organization_id
        || parent.subject != build.subject
        || parent.id != parent_id
        || parent.attempt.checked_add(1) != Some(build.attempt)
        || !parent.status.is_terminal()
    {
        return Err(FlowError::Runtime(
            "parent build cache does not match retry ownership".into(),
        ));
    }
    let Some(output) = parent.box_build_output else {
        return Ok(None);
    };
    output
        .validate()
        .map_err(|error| flow_error("parent Box build output is invalid", error))?;
    Ok(Some(output))
}

pub(super) fn next_poll(
    now: DateTime<Utc>,
    interval: chrono::Duration,
    deadline: DateTime<Utc>,
) -> a3s_flow::Result<DateTime<Utc>> {
    now.checked_add_signed(interval)
        .map(|next| next.min(deadline))
        .ok_or_else(|| FlowError::Runtime("build poll time overflowed".into()))
}

pub(super) fn bounded_reason(reason: impl AsRef<str>) -> String {
    let normalized = reason
        .as_ref()
        .chars()
        .map(|character| {
            if matches!(character, '\0' | '\r' | '\n') {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let normalized = normalized.trim();
    if normalized.is_empty() {
        return "build failed without a provider reason".into();
    }
    normalized.chars().take(16 * 1024).collect()
}
