use super::build_run_log_stream::build_run_log_stream;
use crate::modules::artifacts::application::{
    GetBuildEvidence, GetBuildRun, GetBuildRunLogs, ListBuildRuns,
};
use crate::modules::artifacts::presentation::dto::{
    BuildEvidenceResponse, BuildRunLogsResponse, BuildRunResponse,
};
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{BuildRunId, EnvironmentId, OrganizationId, ProjectId};
use crate::presentation::application_error_response;
use a3s_boot::{BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use a3s_runtime::contract::RuntimeLogStream;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn build_run_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let get_bus = Arc::clone(&bus);
    let get_evidence_bus = Arc::clone(&bus);
    let get_logs_bus = Arc::clone(&bus);
    let stream_logs_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/build-runs",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    let limit = request
                        .optional_query_value_as::<usize>("limit")?
                        .unwrap_or(50);
                    if limit == 0 || limit > 200 {
                        return Err(BootError::BadRequest(
                            "limit must be between 1 and 200".into(),
                        ));
                    }
                    match bus
                        .execute(ListBuildRuns {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            limit,
                        })
                        .await?
                    {
                        Ok(build_runs) => BootResponse::json(
                            &build_runs
                                .into_iter()
                                .map(BuildRunResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/build-runs/{build_run_id}/evidence",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_evidence_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetBuildEvidence {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            build_run_id: BuildRunId::from_uuid(
                                request.param_as::<Uuid>("build_run_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(evidence) => BootResponse::json(&BuildEvidenceResponse::from(evidence)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/build-runs/{build_run_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetBuildRun {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            build_run_id: BuildRunId::from_uuid(
                                request.param_as::<Uuid>("build_run_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(build_run) => BootResponse::json(&BuildRunResponse::from(build_run)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/build-runs/{build_run_id}/logs",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_logs_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let parameters: BuildLogsQuery = request.query()?;
                    match bus
                        .execute(GetBuildRunLogs {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            build_run_id: BuildRunId::from_uuid(
                                request.param_as::<Uuid>("build_run_id")?,
                            ),
                            after_sequence: decode_cursor(parameters.cursor.as_deref())?,
                            limit: parameters.limit,
                            stream: parameters.stream.map(Into::into),
                        })
                        .await?
                    {
                        Ok(logs) => BootResponse::json(&BuildRunLogsResponse::from(logs)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .sse(
            "/{organization_id}/build-runs/{build_run_id}/logs/stream",
            move |request: BootRequest| {
                let bus = Arc::clone(&stream_logs_bus);
                async move {
                    let parameters: BuildLiveLogsQuery = request.query()?;
                    if parameters.limit == 0 || parameters.limit > MAX_LIVE_LOG_RECORDS {
                        return Err(BootError::BadRequest(format!(
                            "live build log limit must be between 1 and {MAX_LIVE_LOG_RECORDS}"
                        )));
                    }
                    let after_sequence = match request
                        .header("last-event-id")
                        .filter(|event_id| !event_id.is_empty())
                    {
                        Some(event_id) => decode_cursor(Some(event_id))?,
                        None => decode_cursor(parameters.cursor.as_deref())?,
                    };
                    build_run_log_stream(
                        bus,
                        GetBuildRunLogs {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            build_run_id: BuildRunId::from_uuid(
                                request.param_as::<Uuid>("build_run_id")?,
                            ),
                            after_sequence,
                            limit: parameters.limit,
                            stream: parameters.stream.map(Into::into),
                        },
                    )
                    .await
                }
            },
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BuildLogsQuery {
    cursor: Option<String>,
    #[serde(default = "default_log_limit")]
    limit: u16,
    stream: Option<BuildLogStreamQuery>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BuildLiveLogsQuery {
    cursor: Option<String>,
    #[serde(default = "default_live_log_limit")]
    limit: u16,
    stream: Option<BuildLogStreamQuery>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BuildLogStreamQuery {
    Stdout,
    Stderr,
}

impl From<BuildLogStreamQuery> for RuntimeLogStream {
    fn from(stream: BuildLogStreamQuery) -> Self {
        match stream {
            BuildLogStreamQuery::Stdout => Self::Stdout,
            BuildLogStreamQuery::Stderr => Self::Stderr,
        }
    }
}

const fn default_log_limit() -> u16 {
    100
}

const MAX_LIVE_LOG_RECORDS: u16 = 16;

const fn default_live_log_limit() -> u16 {
    MAX_LIVE_LOG_RECORDS
}

fn decode_cursor(cursor: Option<&str>) -> Result<Option<u64>> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    cursor
        .strip_prefix("v1:")
        .filter(|sequence| !sequence.is_empty())
        .and_then(|sequence| sequence.parse::<u64>().ok())
        .map(Some)
        .ok_or_else(|| BootError::BadRequest("invalid build log cursor".into()))
}

fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}
