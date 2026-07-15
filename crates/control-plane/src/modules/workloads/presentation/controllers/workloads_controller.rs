use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, OrganizationId, ProjectId,
};
use crate::modules::workloads::application::{
    CancelDeployment, CreateWorkloadDeployment, StopWorkload,
};
use crate::modules::workloads::presentation::dto::{
    CancelDeploymentResponse, CreateWorkloadRequest, WorkloadDeploymentResponse,
    WorkloadStopResponse,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn workloads_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let cancel_bus = Arc::clone(&bus);
    let stop_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::WORKLOAD_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/workloads",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let body: CreateWorkloadRequest = request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                    let environment_id =
                        EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateWorkloadDeployment {
                            organization_id,
                            project_id,
                            environment_id,
                            name: body.name,
                            template: body.template.into_domain(),
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.bundle.replayed { 200 } else { 202 };
                            BootResponse::json_with_status(
                                status,
                                &WorkloadDeploymentResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/workloads/{workload_id}/stop",
            move |request: BootRequest| {
                let bus = Arc::clone(&stop_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let workload_id = crate::modules::shared_kernel::domain::WorkloadId::from_uuid(
                        request.param_as::<Uuid>("workload_id")?,
                    );
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(StopWorkload {
                            organization_id,
                            workload_id,
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.bundle.replayed { 200 } else { 202 };
                            BootResponse::json_with_status(
                                status,
                                &WorkloadStopResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .delete(
            "/{organization_id}/deployments/{deployment_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&cancel_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let deployment_id =
                        DeploymentId::from_uuid(request.param_as::<Uuid>("deployment_id")?);
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CancelDeployment {
                            organization_id,
                            deployment_id,
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.replayed { 200 } else { 202 };
                            BootResponse::json_with_status(
                                status,
                                &CancelDeploymentResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

fn request_identity(request: &BootRequest) -> Result<(String, Uuid)> {
    let idempotency_key = request
        .header("idempotency-key")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BootError::BadRequest("idempotency-key header is required".into()))?
        .to_owned();
    let request_id = request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })?;
    Ok((idempotency_key, request_id))
}
