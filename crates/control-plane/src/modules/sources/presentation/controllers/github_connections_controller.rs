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
    GITHUB_SOURCE_CONNECTION_ROUTE, SOURCES_CONTROLLER_PREFIX,
};
use crate::presentation::{application_error_response, oauth_no_store, OAuthNoStoreErrorFilter};
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn github_connections_controller(
    commands: Arc<CommandBus>,
    queries: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    let begin_commands = commands;
    let connection_queries = Arc::clone(&queries);
    let repository_queries = Arc::clone(&queries);
    ControllerDefinition::new(SOURCES_CONTROLLER_PREFIX)?
        .with_guard(OrganizationTenantGuard)
        .with_filter(OAuthNoStoreErrorFilter)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::SOURCE_WRITE])?
        .post(
            GITHUB_SOURCE_CONNECTION_ROUTE,
            move |request: BootRequest| {
                let commands = Arc::clone(&begin_commands);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let request_id = request_id(&request)?;
                    match commands
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
            },
        )?
        .get(
            GITHUB_SOURCE_CONNECTION_ROUTE,
            move |request: BootRequest| {
                let queries = Arc::clone(&connection_queries);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let request_id = request_id(&request)?;
                    match queries
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
            },
        )?
        .get(
            GITHUB_REPOSITORY_DISCOVERY_ROUTE,
            move |request: BootRequest| {
                let queries = Arc::clone(&repository_queries);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let request_id = request_id(&request)?;
                    match queries
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
            },
        )?
        .get(
            GITHUB_REPOSITORY_REFERENCE_DISCOVERY_ROUTE,
            move |request: BootRequest| {
                let queries = Arc::clone(&queries);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let request_id = request_id(&request)?;
                    match queries
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
            },
        )
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
