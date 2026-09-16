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
    controller, post, BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition,
    Result, RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn secrets_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    // Nest macros own create; rotate/revoke keep deferred project admission.
    // Tenant admission stays on the Secrets entry helper (architecture boundary).
    let rotate_bus = Arc::clone(&bus);
    let revoke_bus = Arc::clone(&bus);
    let mut controller = Arc::new(SecretsController { bus })
        .controller()?;
    controller = controller.route(with_deferred_project_scope(RouteDefinition::post(
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
    )?)?)?;
    controller = controller.route(with_deferred_project_scope(RouteDefinition::post(
        "/{organization_id}/secrets/{secret_id}/versions/{version}/revoke",
        move |request: BootRequest| {
            let bus = Arc::clone(&revoke_bus);
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

#[derive(Debug, Clone)]
struct SecretsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
impl SecretsController {
    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/secrets",
        raw
    )]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateSecretRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let environment_id =
            EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
        let (idempotency_key, request_id) = request_identity(&request)?;
        let access = secret_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        let value = SecretPlaintext::new(body.value).map_err(BootError::BadRequest)?;
        match self
            .bus
            .execute(CreateSecret {
                organization_id,
                project_id,
                environment_id,
                access,
                name: body.name,
                value,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(status, &SecretMutationResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_secrets_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn secrets_controller_registers_create_via_nest_macros_and_deferred_mutations() {
        let controller =
            secrets_controller(Arc::new(CommandBus::new())).expect("secrets nest controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 3);
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Post
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/secrets"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Post
                && route.path()
                    == "/organizations/{organization_id}/secrets/{secret_id}/versions"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Post
                && route.path()
                    == "/organizations/{organization_id}/secrets/{secret_id}/versions/{version}/revoke"
        }));
    }
}
