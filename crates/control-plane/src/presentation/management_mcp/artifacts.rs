use super::arguments::{
    BuildRunArguments, BuildRunListArguments, BuildRunLogArguments, LogStreamArguments,
};
use super::tool_result;
use crate::modules::artifacts::{
    BuildEvidenceResponse, BuildRunLogsResponse, BuildRunResponse, CancelBuildRunResponse,
    RetryBuildRunResponse,
};
use crate::modules::artifacts::{
    BuildLogStream, CancelBuildRun, GetBuildEvidence, GetBuildRun, GetBuildRunLogs, ListBuildRuns,
    RetryBuildRun,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::domain::{BuildRunId, EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CommandBus, QueryBus, Result};
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildRunMutationArguments {
    pub build_run_id: Uuid,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    pub idempotency_key: String,
}

pub async fn list_build_runs(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: BuildRunListArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListBuildRuns {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            limit: arguments.limit,
        })
        .await?
    {
        Ok(build_runs) => tool_result::success(
            200,
            build_runs
                .into_iter()
                .map(BuildRunResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_build_run(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: BuildRunArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetBuildRun {
            organization_id,
            build_run_id: BuildRunId::from_uuid(arguments.build_run_id),
            resource_access,
        })
        .await?
    {
        Ok(build_run) => tool_result::success(200, BuildRunResponse::from(build_run), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_build_run_logs(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: BuildRunLogArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetBuildRunLogs {
            organization_id,
            build_run_id: BuildRunId::from_uuid(arguments.build_run_id),
            resource_access,
            after_sequence: arguments.after_sequence,
            limit: arguments.limit,
            stream: arguments.stream.map(|stream| match stream {
                LogStreamArguments::Stdout => BuildLogStream::Stdout,
                LogStreamArguments::Stderr => BuildLogStream::Stderr,
            }),
        })
        .await?
    {
        Ok(logs) => tool_result::success(200, BuildRunLogsResponse::from(logs), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_build_evidence(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: BuildRunArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetBuildEvidence {
            organization_id,
            build_run_id: BuildRunId::from_uuid(arguments.build_run_id),
            resource_access,
        })
        .await?
    {
        Ok(evidence) => {
            tool_result::success(200, BuildEvidenceResponse::from(evidence), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn cancel_build_run(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    arguments: BuildRunMutationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CancelBuildRun {
            organization_id,
            build_run_id: BuildRunId::from_uuid(arguments.build_run_id),
            resource_access,
            idempotency_key: arguments.idempotency_key,
            requested_at: Utc::now(),
        })
        .await?
    {
        Ok(result) => {
            let status = if result.replayed { 200 } else { 202 };
            tool_result::success(status, CancelBuildRunResponse::from(result), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn retry_build_run(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    arguments: BuildRunMutationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(RetryBuildRun {
            organization_id,
            build_run_id: BuildRunId::from_uuid(arguments.build_run_id),
            resource_access,
            idempotency_key: arguments.idempotency_key,
            requested_at: Utc::now(),
        })
        .await?
    {
        Ok(result) => {
            let status = if result.replayed { 200 } else { 202 };
            tool_result::success(status, RetryBuildRunResponse::from(result), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}
