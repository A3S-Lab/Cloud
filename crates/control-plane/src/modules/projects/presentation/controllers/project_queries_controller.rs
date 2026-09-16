use crate::access_projection::project_access;
use crate::modules::identity::presentation::{resource_access_evaluator, OrganizationTenantGuard};
use crate::modules::projects::application::queries::get_project_attribution::GetProjectAttribution;
use crate::modules::projects::application::queries::list_environments::ListEnvironments;
use crate::modules::projects::application::queries::list_projects::ListProjects;
use crate::modules::projects::presentation::dto::{
    EnvironmentListItemResponse, ProjectAttributionProfileResponse, ProjectListItemResponse,
};
use crate::modules::shared_kernel::domain::{
    OrganizationId, ProjectAttributionProfileId, ProjectId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, get, use_guard, BootError, BootRequest, BootResponse, ControllerDefinition,
    QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn project_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(ProjectQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct ProjectQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
impl ProjectQueriesController {
    #[get("/{organization_id}/projects", raw)]
    async fn list_projects(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let request_id = request_id(&request)?;
        let access = project_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(ListProjects {
                organization_id,
                access,
            })
            .await?
        {
            Ok(projects) => BootResponse::json(
                &projects
                    .into_iter()
                    .map(ProjectListItemResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/projects/{project_id}/attribution-profile", raw)]
    async fn current_attribution_profile(&self, request: BootRequest) -> Result<BootResponse> {
        get_attribution_profile(Arc::clone(&self.bus), request, None).await
    }

    #[get(
        "/{organization_id}/projects/{project_id}/attribution-profiles/{attribution_profile_id}",
        raw
    )]
    async fn attribution_profile_by_id(&self, request: BootRequest) -> Result<BootResponse> {
        let profile_id = ProjectAttributionProfileId::from_uuid(
            request.param_as::<Uuid>("attribution_profile_id")?,
        );
        get_attribution_profile(Arc::clone(&self.bus), request, Some(profile_id)).await
    }
}

async fn get_attribution_profile(
    bus: Arc<QueryBus>,
    request: BootRequest,
    attribution_profile_id: Option<ProjectAttributionProfileId>,
) -> Result<BootResponse> {
    let organization_id = OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
    let request_id = request_id(&request)?;
    let access = project_access(&resource_access_evaluator(
        &request.require_auth_principal()?,
    )?);
    match bus
        .execute(GetProjectAttribution {
            organization_id,
            project_id,
            attribution_profile_id,
            access,
        })
        .await?
    {
        Ok(profile) => BootResponse::json(&ProjectAttributionProfileResponse::from(profile)),
        Err(error) => application_error_response(error, request_id),
    }
}

pub fn environment_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(EnvironmentQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct EnvironmentQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
impl EnvironmentQueriesController {
    #[get("/{organization_id}/projects/{project_id}/environments", raw)]
    async fn list_environments(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let request_id = request_id(&request)?;
        let access = project_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(ListEnvironments {
                organization_id,
                project_id,
                access,
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

#[cfg(test)]
mod nest_macro_project_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;
    use std::collections::HashSet;

    #[test]
    fn project_queries_register_guarded_gets_via_nest_macros() {
        let controller =
            project_queries_controller(Arc::new(QueryBus::new())).expect("project nest queries");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 3);
        assert!(routes.iter().all(|route| route.method() == HttpMethod::Get));
        let paths: HashSet<_> = routes.iter().map(|route| route.path().to_string()).collect();
        assert_eq!(
            paths,
            HashSet::from([
                "/organizations/{organization_id}/projects".to_string(),
                "/organizations/{organization_id}/projects/{project_id}/attribution-profile"
                    .to_string(),
                "/organizations/{organization_id}/projects/{project_id}/attribution-profiles/{attribution_profile_id}"
                    .to_string(),
            ])
        );
    }

    #[test]
    fn environment_queries_register_guarded_get_via_nest_macros() {
        let controller = environment_queries_controller(Arc::new(QueryBus::new()))
            .expect("environment nest queries");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/environments"
        );
    }
}
