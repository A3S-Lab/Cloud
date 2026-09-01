use crate::modules::secrets::application::{
    CreateSecret, RevokeSecretVersion, RotateSecret, SecretPlaintext,
};
use crate::modules::secrets::presentation::dto::{
    CreateSecretRequest, SecretMutationResponse, SecretValueRequest,
};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId, SecretId};
use crate::presentation::{
    application_error_response, organization_tenant_secret_write_controller, request_identity,
    resource_access_evaluator, secret_access, with_deferred_project_scope,
};
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn secrets_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let create_bus = Arc::clone(&bus);
    let rotate_bus = Arc::clone(&bus);
    let controller = ControllerDefinition::new("/organizations")?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/secrets",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_bus);
                async move {
                    let body: CreateSecretRequest = request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                    let environment_id =
                        EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    let value = SecretPlaintext::new(body.value).map_err(BootError::BadRequest)?;
                    match bus
                        .execute(CreateSecret {
                            organization_id,
                            project_id,
                            environment_id,
                            name: body.name,
                            value,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.replayed { 200 } else { 201 };
                            BootResponse::json_with_status(
                                status,
                                &SecretMutationResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .route(with_deferred_project_scope(RouteDefinition::post(
            "/{organization_id}/secrets/{secret_id}/versions",
            move |request: BootRequest| {
                let bus = Arc::clone(&rotate_bus);
                async move {
                    let body: SecretValueRequest = request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let secret_id = SecretId::from_uuid(request.param_as::<Uuid>("secret_id")?);
                    let access = secret_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    let value = SecretPlaintext::new(body.value).map_err(BootError::BadRequest)?;
                    match bus
                        .execute(RotateSecret {
                            organization_id,
                            secret_id,
                            access,
                            value,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.replayed { 200 } else { 201 };
                            BootResponse::json_with_status(
                                status,
                                &SecretMutationResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?)?)?
        .route(with_deferred_project_scope(RouteDefinition::post(
            "/{organization_id}/secrets/{secret_id}/versions/{version}/revoke",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let secret_id = SecretId::from_uuid(request.param_as::<Uuid>("secret_id")?);
                    let access = secret_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    let version = request.param_as::<u64>("version")?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(RevokeSecretVersion {
                            organization_id,
                            secret_id,
                            access,
                            version,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json(&SecretMutationResponse::from(result)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?)?)?;
    organization_tenant_secret_write_controller(controller)
}
