use crate::modules::artifacts::application::{GetBuildRun, ListBuildRuns};
use crate::modules::artifacts::presentation::dto::BuildRunResponse;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{BuildRunId, EnvironmentId, OrganizationId, ProjectId};
use crate::presentation::application_error_response;
use a3s_boot::{BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use std::sync::Arc;
use uuid::Uuid;

pub fn build_run_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let get_bus = Arc::clone(&bus);
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
        )
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
