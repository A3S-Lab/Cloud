use super::delivery_dto::{
    ApplicationDeliveryCredentialExpectedGenerationRequest,
    ApplicationDeliveryCredentialMutationResponse, ApplicationDeliveryCredentialResponse,
    RegisterApplicationDeliveryCredentialRequest,
};
use crate::access_projection::application_access;
use crate::modules::applications::application::{
    DisableApplicationDeliveryCredential, EnableApplicationDeliveryCredential,
    GetApplicationDeliveryCredential, ListApplicationDeliveryCredentials,
    RegisterApplicationDeliveryCredential, RevokeApplicationDeliveryCredential,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{
    ApplicationDeliveryCredentialId, ApplicationId, ApplicationReleaseId, OrganizationId,
    ProjectId, SecretId, SecretVersionReference,
};
use crate::presentation::{
    OrganizationTenantGuard, resource_access_evaluator, actor_principal_id, application_error_response, request_id};
use a3s_boot::{
    controller, get, metadata, post, use_guard, BootRequest, BootResponse, CommandBus,
    ControllerDefinition, QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn application_delivery_credential_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ApplicationDeliveryCredentialCommandsController { bus }).controller()
}

pub fn application_delivery_credential_queries_controller(
    bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ApplicationDeliveryCredentialQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct ApplicationDeliveryCredentialCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct ApplicationDeliveryCredentialQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationDeliveryCredentialCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/delivery-credentials",
        raw
    )]
    async fn register(&self, request: BootRequest) -> Result<BootResponse> {
        let body: RegisterApplicationDeliveryCredentialRequest =
            request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        let secret = match SecretVersionReference::new(
            SecretId::from_uuid(body.secret_id),
            body.secret_version,
        ) {
            Ok(value) => value,
            Err(error) => {
                return application_error_response(
                    crate::modules::shared_kernel::application::ApplicationError::Invalid(error),
                    request_id,
                );
            }
        };
        match self
            .bus
            .execute(RegisterApplicationDeliveryCredential {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                application_release_id: ApplicationReleaseId::from_uuid(
                    body.application_release_id,
                ),
                credential_id: ApplicationDeliveryCredentialId::from_uuid(body.credential_id),
                lookup_key: body.lookup_key,
                secret,
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                created_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 201 },
                &ApplicationDeliveryCredentialMutationResponse::from(result),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/projects/{project_id}/applications/{application_id}/delivery-credentials/{credential_id}/disable", raw)]
    async fn disable(&self, request: BootRequest) -> Result<BootResponse> {
        lifecycle(Arc::clone(&self.bus), request, LifecycleAction::Disable).await
    }

    #[post("/{organization_id}/projects/{project_id}/applications/{application_id}/delivery-credentials/{credential_id}/enable", raw)]
    async fn enable(&self, request: BootRequest) -> Result<BootResponse> {
        lifecycle(Arc::clone(&self.bus), request, LifecycleAction::Enable).await
    }

    #[post("/{organization_id}/projects/{project_id}/applications/{application_id}/delivery-credentials/{credential_id}/revoke", raw)]
    async fn revoke(&self, request: BootRequest) -> Result<BootResponse> {
        lifecycle(Arc::clone(&self.bus), request, LifecycleAction::Revoke).await
    }
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationDeliveryCredentialQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/delivery-credentials",
        raw
    )]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ListApplicationDeliveryCredentials {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(items) => BootResponse::json(
                &items
                    .into_iter()
                    .map(ApplicationDeliveryCredentialResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/delivery-credentials/{credential_id}",
        raw
    )]
    async fn get(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetApplicationDeliveryCredential {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                credential_id: ApplicationDeliveryCredentialId::from_uuid(
                    request.param_as::<Uuid>("credential_id")?,
                ),
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(credential) => {
                BootResponse::json(&ApplicationDeliveryCredentialResponse::from(credential))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

enum LifecycleAction {
    Disable,
    Enable,
    Revoke,
}

async fn lifecycle(
    bus: Arc<CommandBus>,
    request: BootRequest,
    action: LifecycleAction,
) -> Result<BootResponse> {
    let body: ApplicationDeliveryCredentialExpectedGenerationRequest =
        request.json_with_content_type()?;
    let request_id = request_id(&request)?;
    let organization_id = OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
    let application_id = ApplicationId::from_uuid(request.param_as::<Uuid>("application_id")?);
    let credential_id =
        ApplicationDeliveryCredentialId::from_uuid(request.param_as::<Uuid>("credential_id")?);
    let actor_principal_id = actor_principal_id(&request)?;
    let access = application_access(&resource_access_evaluator(
        &request.require_auth_principal()?,
    )?);
    let now = Utc::now();
    let result = match action {
        LifecycleAction::Disable => {
            bus.execute(DisableApplicationDeliveryCredential {
                organization_id,
                project_id,
                application_id,
                credential_id,
                expected_generation: body.expected_generation,
                actor_principal_id,
                access,
                updated_at: now,
            })
            .await?
        }
        LifecycleAction::Enable => {
            bus.execute(EnableApplicationDeliveryCredential {
                organization_id,
                project_id,
                application_id,
                credential_id,
                expected_generation: body.expected_generation,
                actor_principal_id,
                access,
                updated_at: now,
            })
            .await?
        }
        LifecycleAction::Revoke => {
            bus.execute(RevokeApplicationDeliveryCredential {
                organization_id,
                project_id,
                application_id,
                credential_id,
                expected_generation: body.expected_generation,
                actor_principal_id,
                access,
                revoked_at: now,
            })
            .await?
        }
    };
    match result {
        Ok(value) => BootResponse::json_with_status(
            200,
            &ApplicationDeliveryCredentialMutationResponse::from(value),
        ),
        Err(error) => application_error_response(error, request_id),
    }
}

#[cfg(test)]
mod nest_macro_delivery_credential_delivery_controller_tests {
    use super::*;

    #[test]
    fn delivery_credential_controllers_register_scoped_routes_via_nest_macros() {
        let commands =
            application_delivery_credential_commands_controller(Arc::new(CommandBus::new()))
                .expect("commands");
        let queries =
            application_delivery_credential_queries_controller(Arc::new(QueryBus::new()))
                .expect("queries");
        assert_routes_contain(
            &commands,
            &[
                (
                    "POST",
                    "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/delivery-credentials",
                ),
                (
                    "POST",
                    "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/delivery-credentials/{credential_id}/disable",
                ),
                (
                    "POST",
                    "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/delivery-credentials/{credential_id}/enable",
                ),
                (
                    "POST",
                    "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/delivery-credentials/{credential_id}/revoke",
                ),
            ],
        );
        assert_routes_contain(
            &queries,
            &[
                (
                    "GET",
                    "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/delivery-credentials",
                ),
                (
                    "GET",
                    "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/delivery-credentials/{credential_id}",
                ),
            ],
        );
    }

    fn assert_routes_contain(controller: &ControllerDefinition, expected: &[(&str, &str)]) {
        let routes = controller.routes();
        for (method, path) in expected {
            assert!(
                routes.iter().any(|route| {
                    route.method().as_str().eq_ignore_ascii_case(method) && route.path() == *path
                }),
                "missing {method} {path}; have {:?}",
                routes
                    .iter()
                    .map(|route| format!("{} {}", route.method().as_str(), route.path()))
                    .collect::<Vec<_>>(),
            );
        }
    }
}
