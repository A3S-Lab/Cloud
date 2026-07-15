use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, OrganizationId, ProjectId, WorkloadId,
};
use crate::modules::workloads::application::{GetDeployment, GetWorkload, ListWorkloads};
use crate::modules::workloads::presentation::dto::{DeploymentResponse, WorkloadResponse};
use crate::presentation::application_error_response;
use a3s_boot::{BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use std::sync::Arc;
use uuid::Uuid;

pub fn workload_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let get_workload_bus = Arc::clone(&bus);
    let get_deployment_bus = Arc::clone(&bus);
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
