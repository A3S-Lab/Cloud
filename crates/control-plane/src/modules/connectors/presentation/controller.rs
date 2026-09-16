use super::dto::{
    ConnectorExecutionAttemptPageResponse, ConnectorExecutionAttemptResolutionMutationResponse,
    ConnectorExecutionAttemptResolutionResponse, ConnectorExecutionAttemptResponse,
    ConnectorProfileMutationResponse, ConnectorProfileRecordResponse, ConnectorProfileResponse,
    ConnectorRevisionResponse, ConnectorRevisionRevocationMutationResponse,
    ConnectorRevisionRevocationResponse, CreateConnectorProfileRequest,
    ResolveConnectorExecutionAttemptRequest, ReviseConnectorProfileRequest,
    RevokeConnectorRevisionRequest,
};
use super::request::{actor_principal_id, request_id, request_identity};
use crate::access_projection::connector_access;
use crate::modules::connectors::application::{
    CreateConnectorProfile, DEFAULT_CONNECTOR_PROFILE_LIST_LIMIT, GetConnectorExecutionAttempt,
    GetConnectorExecutionAttemptResolution, GetConnectorProfile, GetConnectorRevision,
    GetConnectorRevisionRevocation, ListConnectorProfiles, ListConnectorRevisions,
    ListUnresolvedConnectorExecutionAttempts, MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT,
    ResolveConnectorExecutionAttempt, ReviseConnectorProfile, RevokeConnectorRevision,
};
use crate::modules::connectors::domain::{
    ConnectorExecutionAttemptCursor, DEFAULT_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE,
    MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{OrganizationTenantGuard, resource_access_evaluator};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId, ProjectId,
    canonical_timestamp,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    AUTH_SCOPES_METADATA, BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition,
    QueryBus, Result, controller, get, metadata, post, use_guard,
};
use chrono::Utc;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn connector_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(ConnectorCommandsController { bus }).controller()
}

pub fn connector_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(ConnectorQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct ConnectorCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct ConnectorQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::CONNECTOR_WRITE])]
impl ConnectorCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles",
        raw
    )]
    async fn create_profile(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateConnectorProfileRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateConnectorProfile {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                name: body.name,
                definition_acl: body.definition_acl,
                actor_principal_id: actor_principal_id(&request)?,
                access: connector_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(
                    status,
                    &ConnectorProfileMutationResponse::from(result),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles/{profile_id}/revisions",
        raw
    )]
    async fn revise_profile(&self, request: BootRequest) -> Result<BootResponse> {
        let body: ReviseConnectorProfileRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(ReviseConnectorProfile {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                profile_id: ConnectorProfileId::from_uuid(request.param_as::<Uuid>("profile_id")?),
                expected_version: body.expected_version,
                definition_acl: body.definition_acl,
                actor_principal_id: actor_principal_id(&request)?,
                access: connector_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(
                    status,
                    &ConnectorProfileMutationResponse::from(result),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles/{profile_id}/revisions/{revision_id}/revocation",
        raw
    )]
    async fn revoke_revision(&self, request: BootRequest) -> Result<BootResponse> {
        let body: RevokeConnectorRevisionRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(RevokeConnectorRevision {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                profile_id: ConnectorProfileId::from_uuid(request.param_as::<Uuid>("profile_id")?),
                revision_id: ConnectorRevisionId::from_uuid(
                    request.param_as::<Uuid>("revision_id")?,
                ),
                reason: body.reason,
                actor_principal_id: actor_principal_id(&request)?,
                access: connector_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(
                    status,
                    &ConnectorRevisionRevocationMutationResponse::from(result),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles/{profile_id}/revisions/{revision_id}/execution-attempts/{attempt_id}/resolution",
        raw
    )]
    async fn resolve_attempt(&self, request: BootRequest) -> Result<BootResponse> {
        let body: ResolveConnectorExecutionAttemptRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(ResolveConnectorExecutionAttempt {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                profile_id: ConnectorProfileId::from_uuid(request.param_as::<Uuid>("profile_id")?),
                revision_id: ConnectorRevisionId::from_uuid(
                    request.param_as::<Uuid>("revision_id")?,
                ),
                attempt_id: request.param_as::<Uuid>("attempt_id")?,
                reason: body.reason,
                actor_principal_id: actor_principal_id(&request)?,
                access: connector_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(
                    status,
                    &ConnectorExecutionAttemptResolutionMutationResponse::from(result),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl ConnectorQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles",
        raw
    )]
    async fn list_profiles(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let limit = list_limit(&request)?;
        match self
            .bus
            .execute(ListConnectorProfiles {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                limit,
                access: connector_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(profiles) => BootResponse::json(
                &profiles
                    .into_iter()
                    .map(ConnectorProfileResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles/{profile_id}",
        raw
    )]
    async fn get_profile(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetConnectorProfile {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                profile_id: ConnectorProfileId::from_uuid(request.param_as::<Uuid>("profile_id")?),
                access: connector_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(record) => BootResponse::json(&ConnectorProfileRecordResponse::from(record)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles/{profile_id}/revisions",
        raw
    )]
    async fn list_revisions(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let limit = list_limit(&request)?;
        match self
            .bus
            .execute(ListConnectorRevisions {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                profile_id: ConnectorProfileId::from_uuid(request.param_as::<Uuid>("profile_id")?),
                limit,
                access: connector_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(revisions) => BootResponse::json(
                &revisions
                    .into_iter()
                    .map(ConnectorRevisionResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles/{profile_id}/revisions/{revision_id}",
        raw
    )]
    async fn get_revision(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetConnectorRevision {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                profile_id: ConnectorProfileId::from_uuid(request.param_as::<Uuid>("profile_id")?),
                revision_id: ConnectorRevisionId::from_uuid(
                    request.param_as::<Uuid>("revision_id")?,
                ),
                access: connector_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(revision) => BootResponse::json(&ConnectorRevisionResponse::from(revision)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles/{profile_id}/revisions/{revision_id}/revocation",
        raw
    )]
    async fn get_revocation(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetConnectorRevisionRevocation {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                profile_id: ConnectorProfileId::from_uuid(request.param_as::<Uuid>("profile_id")?),
                revision_id: ConnectorRevisionId::from_uuid(
                    request.param_as::<Uuid>("revision_id")?,
                ),
                access: connector_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(revocation) => {
                BootResponse::json(&ConnectorRevisionRevocationResponse::from(revocation))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles/{profile_id}/revisions/{revision_id}/execution-attempts",
        raw
    )]
    async fn list_attempts(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let parameters: ConnectorAttemptParameters = request.query()?;
        if parameters.limit == 0 || parameters.limit > MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE
        {
            return Err(BootError::BadRequest(format!(
                "Connector execution attempt limit must be between 1 and {MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE}"
            )));
        }
        let after = parameters
            .cursor
            .as_deref()
            .map(ConnectorExecutionAttemptCursor::parse)
            .transpose()
            .map_err(BootError::BadRequest)?;
        match self
            .bus
            .execute(ListUnresolvedConnectorExecutionAttempts {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                profile_id: ConnectorProfileId::from_uuid(request.param_as::<Uuid>("profile_id")?),
                revision_id: ConnectorRevisionId::from_uuid(
                    request.param_as::<Uuid>("revision_id")?,
                ),
                after,
                limit: parameters.limit,
                access: connector_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(page) => BootResponse::json(&ConnectorExecutionAttemptPageResponse::from_page(
                page,
                canonical_timestamp(Utc::now()),
            )),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles/{profile_id}/revisions/{revision_id}/execution-attempts/{attempt_id}",
        raw
    )]
    async fn get_attempt(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetConnectorExecutionAttempt {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                profile_id: ConnectorProfileId::from_uuid(request.param_as::<Uuid>("profile_id")?),
                revision_id: ConnectorRevisionId::from_uuid(
                    request.param_as::<Uuid>("revision_id")?,
                ),
                attempt_id: request.param_as::<Uuid>("attempt_id")?,
                access: connector_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(attempt) => BootResponse::json(&ConnectorExecutionAttemptResponse::from_record(
                attempt,
                canonical_timestamp(Utc::now()),
            )),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles/{profile_id}/revisions/{revision_id}/execution-attempts/{attempt_id}/resolution",
        raw
    )]
    async fn get_attempt_resolution(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetConnectorExecutionAttemptResolution {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                profile_id: ConnectorProfileId::from_uuid(request.param_as::<Uuid>("profile_id")?),
                revision_id: ConnectorRevisionId::from_uuid(
                    request.param_as::<Uuid>("revision_id")?,
                ),
                attempt_id: request.param_as::<Uuid>("attempt_id")?,
                access: connector_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(resolution) => BootResponse::json(
                &ConnectorExecutionAttemptResolutionResponse::from(resolution),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

fn list_limit(request: &BootRequest) -> Result<usize> {
    let limit = request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_CONNECTOR_PROFILE_LIST_LIMIT);
    if limit == 0 || limit > MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConnectorAttemptParameters {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_connector_attempt_limit")]
    limit: usize,
}

const fn default_connector_attempt_limit() -> usize {
    DEFAULT_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE
}

#[cfg(test)]
mod nest_macro_connector_controller_tests {
    use super::*;

    #[test]
    fn connector_controllers_register_scoped_routes_via_nest_macros() {
        let commands =
            connector_commands_controller(Arc::new(CommandBus::new())).expect("connector commands");
        assert_eq!(commands.prefix(), "/organizations");
        assert_eq!(commands.routes().len(), 4);
        assert!(commands.metadata().get(AUTH_SCOPES_METADATA).is_some());

        let queries =
            connector_queries_controller(Arc::new(QueryBus::new())).expect("connector queries");
        assert_eq!(queries.prefix(), "/organizations");
        assert_eq!(queries.routes().len(), 8);
        assert_eq!(
            queries.routes()[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles"
        );
    }
}
