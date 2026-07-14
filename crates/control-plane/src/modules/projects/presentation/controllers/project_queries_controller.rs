use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::projects::application::queries::list_environments::ListEnvironments;
use crate::modules::projects::application::queries::list_projects::ListProjects;
use crate::modules::projects::presentation::dto::{
    EnvironmentListItemResponse, ProjectListItemResponse,
};
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use crate::presentation::application_error_response;
use a3s_boot::{BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use std::sync::Arc;
use uuid::Uuid;

pub fn project_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get(
            "/{organization_id}/projects",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let request_id = request_id(&request)?;
                    match bus.execute(ListProjects { organization_id }).await? {
                        Ok(projects) => BootResponse::json(
                            &projects
                                .into_iter()
                                .map(ProjectListItemResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

pub fn environment_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get(
            "/{organization_id}/projects/{project_id}/environments",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListEnvironments {
                            organization_id,
                            project_id,
                        })
                        .await?
                    {
                        Ok(environments) => BootResponse::json(
                            &environments
                                .into_iter()
                                .map(EnvironmentListItemResponse::from)
                                .collect::<Vec<_>>(),
                        ),
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
