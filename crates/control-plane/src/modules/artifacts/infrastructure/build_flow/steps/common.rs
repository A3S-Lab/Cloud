use super::super::task_spec::{build_cache_key, project_task_spec};
use super::super::types::BuildFlowInput;
use super::super::{flow_error, BuildFlowRuntime};
use crate::modules::artifacts::domain::BuildRun;
use crate::modules::sources::domain::ExternalSourceRevision;
use a3s_flow::FlowError;
use a3s_runtime::contract::RuntimeUnitSpec;
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

pub(super) async fn load_revision(
    runtime: &BuildFlowRuntime,
    build: &BuildRun,
) -> a3s_flow::Result<ExternalSourceRevision> {
    let revision = runtime
        .sources
        .find(build.organization_id, build.source_revision_id)
        .await
        .map_err(|error| flow_error("could not load build source revision", error))?;
    if revision.organization_id != build.organization_id
        || revision.project_id != build.project_id
        || revision.environment_id != build.environment_id
        || revision.id != build.source_revision_id
    {
        return Err(FlowError::Runtime(
            "build source revision does not match persisted build ownership".into(),
        ));
    }
    Ok(revision)
}

pub(super) async fn project_spec(
    runtime: &BuildFlowRuntime,
    build: &BuildRun,
    revision: &ExternalSourceRevision,
) -> a3s_flow::Result<RuntimeUnitSpec> {
    let cache = load_parent_cache(runtime, build, revision).await?;
    project_task_spec(&runtime.config, build, revision, cache.as_ref())
        .map_err(|error| flow_error("could not project build Runtime Task", error))
}

async fn load_parent_cache(
    runtime: &BuildFlowRuntime,
    build: &BuildRun,
    revision: &ExternalSourceRevision,
) -> a3s_flow::Result<Option<crate::modules::artifacts::domain::ValidatedBuildCache>> {
    if !build.cache_required {
        return Ok(None);
    }
    let Some(parent_id) = build.retry_of_build_run_id else {
        return Ok(None);
    };
    let parent = runtime
        .builds
        .find(build.organization_id, parent_id)
        .await
        .map_err(|error| flow_error("could not load parent build cache", error))?;
    if parent.organization_id != build.organization_id
        || parent.project_id != build.project_id
        || parent.environment_id != build.environment_id
        || parent.source_revision_id != build.source_revision_id
        || parent.id != parent_id
        || parent.attempt.checked_add(1) != Some(build.attempt)
        || !parent.status.is_terminal()
    {
        return Err(FlowError::Runtime(
            "parent build cache does not match retry ownership".into(),
        ));
    }
    let Some(cache) = parent.cache else {
        return Ok(None);
    };
    let expected = build_cache_key(&runtime.config, build, revision)
        .map_err(|error| flow_error("could not derive build cache identity", error))?;
    if cache.key != expected {
        return Ok(None);
    }
    Ok(Some(cache))
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

pub(super) fn timestamp_millis(value: DateTime<Utc>) -> a3s_flow::Result<u64> {
    u64::try_from(value.timestamp_millis())
        .map_err(|_| FlowError::Runtime("build Runtime deadline is invalid".into()))
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
