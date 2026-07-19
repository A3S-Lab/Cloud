use super::workload_log_stream::workload_log_stream;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, OrganizationId, ProjectId, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::application::{
    GetDeployment, GetWorkload, GetWorkloadLogs, ListWorkloads,
};
use crate::modules::workloads::presentation::dto::{
    DeploymentResponse, WorkloadLogsResponse, WorkloadResponse,
};
use crate::presentation::application_error_response;
use a3s_boot::{BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use a3s_runtime::contract::RuntimeLogStream;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn workload_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let get_workload_bus = Arc::clone(&bus);
    let get_deployment_bus = Arc::clone(&bus);
    let get_logs_bus = Arc::clone(&bus);
    let stream_logs_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/workloads",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListWorkloads {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(workloads) => BootResponse::json(
                            &workloads
                                .into_iter()
                                .map(WorkloadResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/workloads/{workload_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_workload_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetWorkload {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workload_id: WorkloadId::from_uuid(
                                request.param_as::<Uuid>("workload_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(workload) => BootResponse::json(&WorkloadResponse::from(workload)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/deployments/{deployment_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_deployment_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetDeployment {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            deployment_id: DeploymentId::from_uuid(
                                request.param_as::<Uuid>("deployment_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(deployment) => BootResponse::json(&DeploymentResponse::from(deployment)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/workloads/{workload_id}/revisions/{revision_id}/logs",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_logs_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let parameters: WorkloadLogsQuery = request.query()?;
                    match bus
                        .execute(GetWorkloadLogs {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workload_id: WorkloadId::from_uuid(
                                request.param_as::<Uuid>("workload_id")?,
                            ),
                            revision_id: WorkloadRevisionId::from_uuid(
                                request.param_as::<Uuid>("revision_id")?,
                            ),
                            after_sequence: decode_cursor(parameters.cursor.as_deref())?,
                            limit: parameters.limit,
                            stream: parameters.stream.map(Into::into),
                        })
                        .await?
                    {
                        Ok(logs) => BootResponse::json(&WorkloadLogsResponse::from(logs)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .sse(
            "/{organization_id}/workloads/{workload_id}/revisions/{revision_id}/logs/stream",
            move |request: BootRequest| {
                let bus = Arc::clone(&stream_logs_bus);
                async move {
                    let parameters: WorkloadLiveLogsQuery = request.query()?;
                    if parameters.limit == 0 || parameters.limit > MAX_LIVE_LOG_RECORDS {
                        return Err(BootError::BadRequest(format!(
                            "live workload log limit must be between 1 and {MAX_LIVE_LOG_RECORDS}"
                        )));
                    }
                    let after_sequence = match request
                        .header("last-event-id")
                        .filter(|event_id| !event_id.is_empty())
                    {
                        Some(event_id) => decode_cursor(Some(event_id))?,
                        None => decode_cursor(parameters.cursor.as_deref())?,
                    };
                    workload_log_stream(
                        bus,
                        GetWorkloadLogs {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workload_id: WorkloadId::from_uuid(
                                request.param_as::<Uuid>("workload_id")?,
                            ),
                            revision_id: WorkloadRevisionId::from_uuid(
                                request.param_as::<Uuid>("revision_id")?,
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
struct WorkloadLogsQuery {
    cursor: Option<String>,
    #[serde(default = "default_log_limit")]
    limit: u16,
    stream: Option<WorkloadLogStreamQuery>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkloadLiveLogsQuery {
    cursor: Option<String>,
    #[serde(default = "default_live_log_limit")]
    limit: u16,
    stream: Option<WorkloadLogStreamQuery>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WorkloadLogStreamQuery {
    Stdout,
    Stderr,
}

impl From<WorkloadLogStreamQuery> for RuntimeLogStream {
    fn from(stream: WorkloadLogStreamQuery) -> Self {
        match stream {
            WorkloadLogStreamQuery::Stdout => Self::Stdout,
            WorkloadLogStreamQuery::Stderr => Self::Stderr,
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
        .ok_or_else(|| BootError::BadRequest("invalid workload log cursor".into()))
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
