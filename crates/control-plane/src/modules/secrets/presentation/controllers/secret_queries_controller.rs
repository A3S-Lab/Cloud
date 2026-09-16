use crate::modules::secrets::application::{GetSecret, ListSecrets};
use crate::modules::secrets::presentation::dto::{SecretDetailsResponse, SecretListItemResponse};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId, SecretId};
use crate::presentation::{
    application_error_response, organization_tenant_secret_read_controller, request_id,
    resource_access_evaluator, secret_access, with_deferred_project_scope,
};
use a3s_boot::{
    controller, get, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result,
    RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn secret_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    // List uses Nest macros; org-scoped get keeps deferred project admission.
    // Tenant admission stays on the Secrets entry helper (architecture boundary).
    let mut controller = Arc::new(SecretQueriesController {
        bus: Arc::clone(&bus),
    })
    .controller()?;
    controller = controller.route(with_deferred_project_scope(RouteDefinition::get(
        "/{organization_id}/secrets/{secret_id}",
        move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let secret_id = SecretId::from_uuid(request.param_as::<Uuid>("secret_id")?);
                let access = secret_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?);
                let request_id = request_id(&request)?;
                match bus
                    .execute(GetSecret {
                        organization_id,
                        secret_id,
                        access,
                    })
                    .await?
                {
                    Ok(secret) => BootResponse::json(&SecretDetailsResponse::from(secret)),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?)?)?;
    organization_tenant_secret_read_controller(controller)
}

#[derive(Debug, Clone)]
struct SecretQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
impl SecretQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/secrets",
        raw
    )]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let environment_id =
            EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
        let request_id = request_id(&request)?;
        let access = secret_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(ListSecrets {
                organization_id,
                project_id,
                environment_id,
                access,
            })
            .await?
        {
            Ok(secrets) => BootResponse::json(
                &secrets
                    .into_iter()
                    .map(SecretListItemResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_secret_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn secret_queries_controller_registers_list_via_nest_macros_and_deferred_get() {
        let controller = secret_queries_controller(Arc::new(QueryBus::new()))
            .expect("secret nest query controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/secrets"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path() == "/organizations/{organization_id}/secrets/{secret_id}"
        }));
    }
}
