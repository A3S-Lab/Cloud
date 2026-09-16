use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::modules::sources::presentation::dto::{
    GithubConnectionInstallResponse, GithubConnectionResponse,
    GithubRepositoryDiscoveryPageResponse, GithubRepositoryReferenceDiscoveryPageResponse,
};
use crate::modules::sources::{
    BeginGithubConnection, GetGithubConnection, ListGithubInstallationRepositories,
    ListGithubRepositoryReferences, DEFAULT_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE,
    GITHUB_REPOSITORY_DISCOVERY_ROUTE, GITHUB_REPOSITORY_REFERENCE_DISCOVERY_ROUTE,
    GITHUB_SOURCE_CONNECTION_ROUTE,
};
use crate::presentation::{application_error_response, oauth_no_store, OAuthNoStoreErrorFilter};
use a3s_boot::{
    controller, get, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootError, BootRequest,
    BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn github_connections_controller(
    commands: Arc<CommandBus>,
    queries: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    // Nest macros own the org-tenant GitHub connection HTTP; OAuth no-store
    // filter stays wiring-owned because it is not a Nest attribute today.
    Ok(Arc::new(GithubConnectionsController { commands, queries })
        .controller()?
        .with_filter(OAuthNoStoreErrorFilter))
}

#[derive(Debug, Clone)]
struct GithubConnectionsController {
    commands: Arc<CommandBus>,
    queries: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::SOURCE_WRITE])]
impl GithubConnectionsController {
    #[post("/{organization_id}/source-connections/github", raw)]
    async fn begin(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let request_id = request_id(&request)?;
        match self
            .commands
            .execute(BeginGithubConnection {
                organization_id,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => Ok(oauth_no_store(BootResponse::json_with_status(
                201,
                &GithubConnectionInstallResponse::from(result),
            )?)),
            Err(error) => Ok(oauth_no_store(application_error_response(
                error, request_id,
            )?)),
        }
    }

    #[get("/{organization_id}/source-connections/github", raw)]
    async fn get_connection(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let request_id = request_id(&request)?;
        match self
            .queries
            .execute(GetGithubConnection { organization_id })
            .await?
        {
            Ok(connection) => Ok(oauth_no_store(BootResponse::json(
                &GithubConnectionResponse::from(connection),
            )?)),
            Err(error) => Ok(oauth_no_store(application_error_response(
                error, request_id,
            )?)),
        }
    }

    #[get(
        "/{organization_id}/source-connections/github/repositories",
        raw
    )]
    async fn list_repositories(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let request_id = request_id(&request)?;
        match self
            .queries
            .execute(ListGithubInstallationRepositories {
                organization_id,
                cursor: request.query_value("cursor")?,
                limit: discovery_limit(&request)?,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(page) => Ok(oauth_no_store(BootResponse::json(
                &GithubRepositoryDiscoveryPageResponse::from(page),
            )?)),
            Err(error) => Ok(oauth_no_store(application_error_response(
                error, request_id,
            )?)),
        }
    }

    #[get(
        "/{organization_id}/source-connections/github/repository-references",
        raw
    )]
    async fn list_repository_references(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let request_id = request_id(&request)?;
        match self
            .queries
            .execute(ListGithubRepositoryReferences {
                organization_id,
                repository_url: required_query_value(&request, "repositoryUrl")?,
                kind: required_query_value(&request, "kind")?,
                cursor: request.query_value("cursor")?,
                limit: discovery_limit(&request)?,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(page) => Ok(oauth_no_store(BootResponse::json(
                &GithubRepositoryReferenceDiscoveryPageResponse::from(page),
            )?)),
            Err(error) => Ok(oauth_no_store(application_error_response(
                error, request_id,
            )?)),
        }
    }
}

fn discovery_limit(request: &BootRequest) -> Result<usize> {
    Ok(request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE))
}

fn required_query_value(request: &BootRequest, name: &str) -> Result<String> {
    request
        .query_value(name)?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BootError::BadRequest(format!("{name} query parameter is required")))
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
mod nest_macro_github_connections_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn github_connections_controller_registers_org_tenant_routes_via_nest_macros() {
        let controller = github_connections_controller(
            Arc::new(CommandBus::new()),
            Arc::new(QueryBus::new()),
        )
        .expect("github connections nest controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 4);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(
            routes[0].path(),
            format!("/organizations{GITHUB_SOURCE_CONNECTION_ROUTE}")
        );
        assert_eq!(routes[1].method(), HttpMethod::Get);
        assert_eq!(
            routes[1].path(),
            format!("/organizations{GITHUB_SOURCE_CONNECTION_ROUTE}")
        );
        assert_eq!(routes[2].method(), HttpMethod::Get);
        assert_eq!(
            routes[2].path(),
            format!("/organizations{GITHUB_REPOSITORY_DISCOVERY_ROUTE}")
        );
        assert_eq!(routes[3].method(), HttpMethod::Get);
        assert_eq!(
            routes[3].path(),
            format!("/organizations{GITHUB_REPOSITORY_REFERENCE_DISCOVERY_ROUTE}")
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::SOURCE_WRITE])
        );
    }
}
