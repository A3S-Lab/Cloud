use crate::access_projection::project_access;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::projects::application::commands::create_environment::CreateEnvironment;
use crate::modules::projects::application::commands::create_project::CreateProject;
use crate::modules::projects::application::commands::update_project_attribution::UpdateProjectAttribution;
use crate::modules::projects::presentation::dto::{
    CreateEnvironmentRequest, CreateProjectRequest, EnvironmentResponse,
    ProjectAttributionMutationResponse, ProjectResponse, UpdateProjectAttributionRequest,
};
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use crate::presentation::{OrganizationTenantGuard, authenticated_actor, resource_access_evaluator, application_error_response};
use a3s_boot::{
    controller, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootError, BootRequest,
    BootResponse, CommandBus, ControllerDefinition, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn projects_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(ProjectsController { bus }).controller()
}

pub fn environments_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(EnvironmentsController { bus }).controller()
}

#[derive(Debug, Clone)]
struct ProjectsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::PROJECT_WRITE])]
impl ProjectsController {
    #[post("/{organization_id}/projects", raw)]
    async fn create_project(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateProjectRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let access = project_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateProject {
                organization_id,
                access,
                name: body.name,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(status, &ProjectResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/attribution-profiles",
        raw
    )]
    async fn update_attribution(&self, request: BootRequest) -> Result<BootResponse> {
        let body: UpdateProjectAttributionRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let principal = request.require_auth_principal()?;
        let actor_principal_id = authenticated_actor(&principal)?.principal_id;
        let access = project_access(&resource_access_evaluator(&principal)?);
        let expected_project_version = expected_version(&request)?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(UpdateProjectAttribution {
                organization_id,
                project_id,
                actor_principal_id,
                access,
                expected_project_version,
                business_owner_reference: body.business_owner_reference,
                cost_attribution_code: body.cost_attribution_code,
                labels: body.labels,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(
                    status,
                    &ProjectAttributionMutationResponse::from(result),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[derive(Debug, Clone)]
struct EnvironmentsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::ENVIRONMENT_WRITE])]
impl EnvironmentsController {
    #[post("/{organization_id}/projects/{project_id}/environments", raw)]
    async fn create_environment(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateEnvironmentRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let access = project_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateEnvironment {
                organization_id,
                project_id,
                access,
                name: body.name,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(status, &EnvironmentResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
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

fn expected_version(request: &BootRequest) -> Result<u64> {
    let version = request
        .header("x-a3s-expected-version")
        .ok_or_else(|| BootError::BadRequest("x-a3s-expected-version header is required".into()))?
        .parse::<u64>()
        .map_err(|_| {
            BootError::BadRequest("x-a3s-expected-version must be a positive integer".into())
        })?;
    if version == 0 {
        return Err(BootError::BadRequest(
            "x-a3s-expected-version must be a positive integer".into(),
        ));
    }
    Ok(version)
}

#[cfg(test)]
mod nest_macro_projects_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn projects_controller_registers_scoped_posts_via_nest_macros() {
        let controller = projects_controller(Arc::new(CommandBus::new()))
            .expect("projects nest command controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(routes[0].path(), "/organizations/{organization_id}/projects");
        assert_eq!(
            routes[1].path(),
            "/organizations/{organization_id}/projects/{project_id}/attribution-profiles"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::PROJECT_WRITE])
        );
    }

    #[test]
    fn environments_controller_registers_scoped_posts_via_nest_macros() {
        let controller = environments_controller(Arc::new(CommandBus::new()))
            .expect("environments nest command controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/environments"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::ENVIRONMENT_WRITE])
        );
    }
}
