use super::dto::{
    ApplicationMutationResponse, ApplicationReleaseResponse, ApplicationResponse,
    CreateApplicationRequest, PublishApplicationReleaseRequest,
};
use crate::access_projection::application_access;
use crate::modules::applications::application::{
    CreateApplication, DEFAULT_APPLICATION_LIST_LIMIT, GetApplication, GetApplicationRelease,
    ListApplicationReleases, ListApplications, MAXIMUM_APPLICATION_LIST_LIMIT,
    PublishApplicationRelease,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{OrganizationTenantGuard, resource_access_evaluator};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationReleaseId, OrganizationId, ProjectId,
};
use crate::presentation::{
    actor_principal_id, application_error_response, request_id, request_identity,
};
use a3s_boot::{
    AUTH_SCOPES_METADATA, BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition,
    QueryBus, Result, controller, get, metadata, post, use_guard,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn application_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(ApplicationCommandsController { bus }).controller()
}

pub fn application_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(ApplicationQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct ApplicationCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct ApplicationQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationCommandsController {
    #[post("/{organization_id}/projects/{project_id}/applications", raw)]
    async fn create_application(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateApplicationRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateApplication {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                name: body.name,
                description: body.description,
                release_acl: body.release_acl,
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(status, &ApplicationMutationResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/releases",
        raw
    )]
    async fn publish_release(&self, request: BootRequest) -> Result<BootResponse> {
        let body: PublishApplicationReleaseRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(PublishApplicationRelease {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                expected_version: body.expected_version,
                release_acl: body.release_acl,
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(status, &ApplicationMutationResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl ApplicationQueriesController {
    #[get("/{organization_id}/projects/{project_id}/applications", raw)]
    async fn list_applications(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let limit = list_limit(&request)?;
        match self
            .bus
            .execute(ListApplications {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                limit: Some(limit),
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(applications) => BootResponse::json(
                &applications
                    .into_iter()
                    .map(ApplicationResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}",
        raw
    )]
    async fn get_application(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetApplication {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(application) => BootResponse::json(&ApplicationResponse::from(application)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/releases",
        raw
    )]
    async fn list_releases(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let limit = list_limit(&request)?;
        match self
            .bus
            .execute(ListApplicationReleases {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                limit: Some(limit),
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(releases) => BootResponse::json(
                &releases
                    .into_iter()
                    .map(ApplicationReleaseResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/releases/{release_id}",
        raw
    )]
    async fn get_release(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetApplicationRelease {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                release_id: ApplicationReleaseId::from_uuid(
                    request.param_as::<Uuid>("release_id")?,
                ),
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(release) => BootResponse::json(&ApplicationReleaseResponse::from(release)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

fn list_limit(request: &BootRequest) -> Result<usize> {
    let limit = request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_APPLICATION_LIST_LIMIT);
    if limit == 0 || limit > MAXIMUM_APPLICATION_LIST_LIMIT {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAXIMUM_APPLICATION_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}

#[cfg(test)]
mod nest_macro_application_controller_tests {
    use super::*;

    #[test]
    fn application_controllers_register_scoped_routes_via_nest_macros() {
        let commands = application_commands_controller(Arc::new(CommandBus::new()))
            .expect("application commands");
        assert_eq!(commands.prefix(), "/organizations");
        assert_eq!(commands.routes().len(), 2);
        assert!(commands.metadata().get(AUTH_SCOPES_METADATA).is_some());

        let queries =
            application_queries_controller(Arc::new(QueryBus::new())).expect("application queries");
        assert_eq!(queries.prefix(), "/organizations");
        assert_eq!(queries.routes().len(), 4);
        assert_eq!(
            queries.routes()[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/applications"
        );
    }
}
