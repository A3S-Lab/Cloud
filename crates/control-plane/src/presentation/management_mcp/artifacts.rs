use super::arguments::{BuildRunArguments, BuildRunListArguments, BuildRunLogArguments};
use super::tool_result;
use crate::modules::artifacts::presentation::{
    BuildEvidenceResponse, BuildRunLogsResponse, BuildRunResponse,
};
use crate::modules::artifacts::{GetBuildEvidence, GetBuildRun, GetBuildRunLogs, ListBuildRuns};
use crate::modules::shared_kernel::domain::{BuildRunId, EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{QueryBus, Result};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

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
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetBuildRun {
            organization_id,
            build_run_id: BuildRunId::from_uuid(arguments.build_run_id),
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
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetBuildRunLogs {
            organization_id,
            build_run_id: BuildRunId::from_uuid(arguments.build_run_id),
            after_sequence: arguments.after_sequence,
            limit: arguments.limit,
            stream: arguments.stream.map(Into::into),
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
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetBuildEvidence {
            organization_id,
            build_run_id: BuildRunId::from_uuid(arguments.build_run_id),
        })
        .await?
    {
        Ok(evidence) => {
            tool_result::success(200, BuildEvidenceResponse::from(evidence), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}
