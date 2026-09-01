use super::request::workload_access;
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, OrganizationId, ProjectId, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::application::{
    GetDeployment, GetWorkload, GetWorkloadLogs, ListWorkloads,
};
use crate::modules::workloads::presentation::dto::{
    DeploymentResponse, WorkloadLogsResponse, WorkloadResponse,
};
use crate::presentation::{
    application_error_response, decode_sequence_cursor, default_live_sequence_limit,
    organization_tenant_workload_read_controller, request_id, resolve_sequence_cursor,
    sequence_stream_error, stream_sequence_pages, with_deferred_project_scope,
    MAX_LIVE_SEQUENCE_RECORDS,
};
use a3s_boot::{
    BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result, RouteDefinition,
    SseStream,
};
use a3s_runtime::contract::RuntimeLogStream;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn workload_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let get_workload_bus = Arc::clone(&bus);
    let get_deployment_bus = Arc::clone(&bus);
    let get_logs_bus = Arc::clone(&bus);
    let stream_logs_bus = Arc::clone(&bus);
    let controller = ControllerDefinition::new("/organizations")?
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
        .route(with_deferred_project_scope(
            RouteDefinition::get(
                "/{organization_id}/workloads/{workload_id}",
                move |request: BootRequest| {
                let bus = Arc::clone(&get_workload_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let access = workload_access(&request)?;
                    match bus
                        .execute(GetWorkload {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workload_id: WorkloadId::from_uuid(
                                request.param_as::<Uuid>("workload_id")?,
                            ),
                            access,
                        })
                        .await?
                    {
                        Ok(workload) => BootResponse::json(&WorkloadResponse::from(workload)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
                },
            )?,
        )?)?
        .route(with_deferred_project_scope(
            RouteDefinition::get(
                "/{organization_id}/deployments/{deployment_id}",
                move |request: BootRequest| {
                let bus = Arc::clone(&get_deployment_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let access = workload_access(&request)?;
                    match bus
                        .execute(GetDeployment {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            deployment_id: DeploymentId::from_uuid(
                                request.param_as::<Uuid>("deployment_id")?,
                            ),
                            access,
                        })
                        .await?
                    {
                        Ok(deployment) => BootResponse::json(&DeploymentResponse::from(deployment)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
                },
            )?,
        )?)?
        .route(with_deferred_project_scope(
            RouteDefinition::get(
                "/{organization_id}/workloads/{workload_id}/revisions/{revision_id}/logs",
                move |request: BootRequest| {
                let bus = Arc::clone(&get_logs_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let access = workload_access(&request)?;
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
                            access,
                            after_sequence: decode_sequence_cursor(
                                parameters.cursor.as_deref(),
                                "workload log",
                            )?,
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
            )?,
        )?)?
        .route(with_deferred_project_scope(
            RouteDefinition::sse(
                "/{organization_id}/workloads/{workload_id}/revisions/{revision_id}/logs/stream",
                move |request: BootRequest| {
                let bus = Arc::clone(&stream_logs_bus);
                async move {
                    let parameters: WorkloadLiveLogsQuery = request.query()?;
                    if parameters.limit == 0 || parameters.limit > MAX_LIVE_SEQUENCE_RECORDS {
                        return Err(BootError::BadRequest(format!(
                            "live workload log limit must be between 1 and {MAX_LIVE_SEQUENCE_RECORDS}"
                        )));
                    }
                    let after_sequence = resolve_sequence_cursor(
                        &request,
                        parameters.cursor.as_deref(),
                        "workload log",
                    )?;
                    let access = workload_access(&request)?;
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
                            access,
                            after_sequence,
                            limit: parameters.limit,
                            stream: parameters.stream.map(Into::into),
                        },
                    )
                    .await
                }
                },
            )?,
        )?)?;
    organization_tenant_workload_read_controller(controller)
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
    #[serde(default = "default_live_sequence_limit")]
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

async fn workload_log_stream(bus: Arc<QueryBus>, query: GetWorkloadLogs) -> Result<SseStream> {
    stream_sequence_pages(
        query,
        move |query| {
            let bus = Arc::clone(&bus);
            async move {
                bus.execute(query)
                    .await?
                    .map(WorkloadLogsResponse::from)
                    .map_err(|error| sequence_stream_error(error, "live workload log query failed"))
            }
        },
        |query, sequence| query.after_sequence = Some(sequence),
        "workload log",
    )
    .await
}
