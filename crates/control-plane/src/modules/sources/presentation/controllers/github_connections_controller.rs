use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::modules::sources::presentation::controllers::github_response_security::{
    no_store, GithubNoStoreErrorFilter,
};
use crate::modules::sources::presentation::dto::{
    GithubConnectionInstallResponse, GithubConnectionResponse,
};
use crate::modules::sources::{BeginGithubConnection, GetGithubConnection};
use crate::presentation::application_error_response;
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
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_filter(GithubNoStoreErrorFilter)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::SOURCE_WRITE])?
        .post(
            "/{organization_id}/source-connections/github",
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
                        Ok(result) => Ok(no_store(BootResponse::json_with_status(
                            201,
                            &GithubConnectionInstallResponse::from(result),
                        )?)),
                        Err(error) => Ok(no_store(application_error_response(error, request_id)?)),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/source-connections/github",
            move |request: BootRequest| {
                let queries = Arc::clone(&queries);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let request_id = request_id(&request)?;
                    match queries
                        .execute(GetGithubConnection { organization_id })
                        .await?
                    {
                        Ok(connection) => Ok(no_store(BootResponse::json(
                            &GithubConnectionResponse::from(connection),
                        )?)),
                        Err(error) => Ok(no_store(application_error_response(error, request_id)?)),
                    }
                }
            },
        )
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
