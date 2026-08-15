use super::dto::{
    ConnectorProfileMutationResponse, ConnectorProfileRecordResponse, ConnectorProfileResponse,
    ConnectorRevisionResponse, CreateConnectorProfileRequest, ReviseConnectorProfileRequest,
};
use super::request::{actor_principal_id, request_id, request_identity};
use crate::modules::connectors::application::{
    CreateConnectorProfile, GetConnectorProfile, GetConnectorRevision, ListConnectorProfiles,
    ListConnectorRevisions, ReviseConnectorProfile, DEFAULT_CONNECTOR_PROFILE_LIST_LIMIT,
    MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{resource_access_evaluator, OrganizationTenantGuard};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId, ProjectId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn connector_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let create_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CONNECTOR_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_bus);
                async move {
                    let body: CreateConnectorProfileRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateConnectorProfile {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            name: body.name,
                            definition_acl: body.definition_acl,
                            actor_principal_id: actor_principal_id(&request)?,
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
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
            },
        )?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles/{profile_id}/revisions",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let body: ReviseConnectorProfileRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(ReviseConnectorProfile {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            profile_id: ConnectorProfileId::from_uuid(
                                request.param_as::<Uuid>("profile_id")?,
                            ),
                            expected_version: body.expected_version,
                            definition_acl: body.definition_acl,
                            actor_principal_id: actor_principal_id(&request)?,
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
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
            },
        )
}

pub fn connector_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_profiles_bus = Arc::clone(&bus);
    let get_profile_bus = Arc::clone(&bus);
    let list_revisions_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_profiles_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let limit = list_limit(&request)?;
                    match bus
                        .execute(ListConnectorProfiles {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            limit,
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
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
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles/{profile_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_profile_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetConnectorProfile {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            profile_id: ConnectorProfileId::from_uuid(
                                request.param_as::<Uuid>("profile_id")?,
                            ),
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
                        })
                        .await?
                    {
                        Ok(record) => {
                            BootResponse::json(&ConnectorProfileRecordResponse::from(record))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles/{profile_id}/revisions",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_revisions_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let limit = list_limit(&request)?;
                    match bus
                        .execute(ListConnectorRevisions {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            profile_id: ConnectorProfileId::from_uuid(
                                request.param_as::<Uuid>("profile_id")?,
                            ),
                            limit,
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
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
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles/{profile_id}/revisions/{revision_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetConnectorRevision {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            profile_id: ConnectorProfileId::from_uuid(
                                request.param_as::<Uuid>("profile_id")?,
                            ),
                            revision_id: ConnectorRevisionId::from_uuid(
                                request.param_as::<Uuid>("revision_id")?,
                            ),
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
                        })
                        .await?
                    {
                        Ok(revision) => {
                            BootResponse::json(&ConnectorRevisionResponse::from(revision))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
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
