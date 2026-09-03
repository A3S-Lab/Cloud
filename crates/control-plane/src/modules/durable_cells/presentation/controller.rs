use super::deployment_admission::DeployDurableCellApplicationFromAcl;
use super::dto::{
    CreateDurableCellApplicationRequest, DeployDurableCellApplicationRequest,
    DurableCellApplicationMutationResponse, DurableCellApplicationRecordResponse,
    DurableCellApplicationResponse, DurableCellApplicationRevisionResponse,
    DurableCellDeploymentResponse, DurableCellRoutePublicationResponse,
    PublishDurableCellApplicationRouteRequest, ReviseDurableCellApplicationRequest,
    SetDurableCellApplicationStateRequest,
};
use super::request::{actor_principal_id, request_id, request_identity};
use crate::modules::durable_cells::{
    CreateDurableCellApplication, GetDurableCellApplication, GetDurableCellApplicationRevision,
    ListDurableCellApplicationRevisions, ListDurableCellApplications,
    PublishDurableCellApplicationRoute, ReviseDurableCellApplication, StartDurableCellApplication,
    StopDurableCellApplication, DEFAULT_DURABLE_CELL_APPLICATION_LIST_LIMIT,
    MAXIMUM_DURABLE_CELL_APPLICATION_LIST_LIMIT,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{resource_access_evaluator, OrganizationTenantGuard};
use crate::modules::shared_kernel::domain::{
    DomainClaimId, DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId,
    GatewayScopeId, OrganizationId, ProjectId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn durable_cell_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let create_bus = Arc::clone(&bus);
    let revise_bus = Arc::clone(&bus);
    let start_bus = Arc::clone(&bus);
    let stop_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::WORKLOAD_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/durable-cell-applications",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_bus);
                async move {
                    let body: CreateDurableCellApplicationRequest =
                        request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    let (organization_id, project_id, environment_id) = scope(&request)?;
                    match bus
                        .execute(CreateDurableCellApplication {
                            organization_id,
                            project_id,
                            environment_id,
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
                        Ok(result) => BootResponse::json_with_status(
                            if result.replayed { 200 } else { 201 },
                            &DurableCellApplicationMutationResponse::from(result),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/durable-cell-applications/{application_id}/revisions",
            move |request: BootRequest| {
                let bus = Arc::clone(&revise_bus);
                async move {
                    let body: ReviseDurableCellApplicationRequest =
                        request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    let (organization_id, project_id, environment_id) = scope(&request)?;
                    match bus
                        .execute(ReviseDurableCellApplication {
                            organization_id,
                            project_id,
                            environment_id,
                            application_id: application_id(&request)?,
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
                        Ok(result) => BootResponse::json_with_status(
                            if result.replayed { 200 } else { 201 },
                            &DurableCellApplicationMutationResponse::from(result),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/durable-cell-applications/{application_id}/start",
            move |request: BootRequest| {
                let bus = Arc::clone(&start_bus);
                async move {
                    let body: SetDurableCellApplicationStateRequest =
                        request.json_with_content_type()?;
                    execute_state(bus, request, body, true).await
                }
            },
        )?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/durable-cell-applications/{application_id}/stop",
            move |request: BootRequest| {
                let bus = Arc::clone(&stop_bus);
                async move {
                    let body: SetDurableCellApplicationStateRequest =
                        request.json_with_content_type()?;
                    execute_state(bus, request, body, false).await
                }
            },
        )?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/durable-cell-applications/{application_id}/revisions/{revision_id}/deployments",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let body: DeployDurableCellApplicationRequest =
                        request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    let (organization_id, project_id, environment_id) = scope(&request)?;
                    match bus
                        .execute(DeployDurableCellApplicationFromAcl {
                            organization_id,
                            project_id,
                            environment_id,
                            application_id: application_id(&request)?,
                            application_revision_id: revision_id(&request)?,
                            service_profile_acl: body.service_profile_acl,
                            storage_provider_profile_acl: body.storage_provider_profile_acl,
                            provider_workload_acl: body.provider_workload_acl,
                            storage_binding_acl: body.storage_binding_acl,
                            actor_principal_id: actor_principal_id(&request)?,
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json_with_status(
                            if result.replayed { 200 } else { 201 },
                            &DurableCellDeploymentResponse::from(result),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

pub fn durable_cell_route_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::ROUTE_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/durable-cell-applications/{application_id}/revisions/{revision_id}/routes",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let body: PublishDurableCellApplicationRouteRequest =
                        request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    let (organization_id, project_id, environment_id) = scope(&request)?;
                    match bus
                        .execute(PublishDurableCellApplicationRoute {
                            organization_id,
                            project_id,
                            environment_id,
                            application_id: application_id(&request)?,
                            application_revision_id: revision_id(&request)?,
                            service_profile_acl: body.service_profile_acl,
                            gateway_scope_id: GatewayScopeId::from_uuid(body.gateway_scope_id),
                            domain_claim_id: DomainClaimId::from_uuid(body.domain_claim_id),
                            hostname: body.hostname,
                            path_prefix: body.path_prefix,
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.publication.replayed {
                                200
                            } else {
                                201
                            };
                            BootResponse::json_with_status(
                                status,
                                &DurableCellRoutePublicationResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

pub fn durable_cell_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_bus = Arc::clone(&bus);
    let get_bus = Arc::clone(&bus);
    let revisions_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/durable-cell-applications",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let (organization_id, project_id, environment_id) = scope(&request)?;
                    match bus
                        .execute(ListDurableCellApplications {
                            organization_id,
                            project_id,
                            environment_id,
                            limit: list_limit(&request)?,
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
                        })
                        .await?
                    {
                        Ok(applications) => BootResponse::json(
                            &applications
                                .into_iter()
                                .map(DurableCellApplicationResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/durable-cell-applications/{application_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let (organization_id, project_id, environment_id) = scope(&request)?;
                    match bus
                        .execute(GetDurableCellApplication {
                            organization_id,
                            project_id,
                            environment_id,
                            application_id: application_id(&request)?,
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
                        })
                        .await?
                    {
                        Ok(record) => BootResponse::json(
                            &DurableCellApplicationRecordResponse::from(record),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/durable-cell-applications/{application_id}/revisions",
            move |request: BootRequest| {
                let bus = Arc::clone(&revisions_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let (organization_id, project_id, environment_id) = scope(&request)?;
                    match bus
                        .execute(ListDurableCellApplicationRevisions {
                            organization_id,
                            project_id,
                            environment_id,
                            application_id: application_id(&request)?,
                            limit: list_limit(&request)?,
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
                        })
                        .await?
                    {
                        Ok(revisions) => BootResponse::json(
                            &revisions
                                .into_iter()
                                .map(DurableCellApplicationRevisionResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/durable-cell-applications/{application_id}/revisions/{revision_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    let (organization_id, project_id, environment_id) = scope(&request)?;
                    match bus
                        .execute(GetDurableCellApplicationRevision {
                            organization_id,
                            project_id,
                            environment_id,
                            application_id: application_id(&request)?,
                            revision_id: revision_id(&request)?,
                            resource_access: resource_access_evaluator(
                                &request.require_auth_principal()?,
                            )?,
                        })
                        .await?
                    {
                        Ok(revision) => BootResponse::json(
                            &DurableCellApplicationRevisionResponse::from(revision),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

async fn execute_state(
    bus: Arc<CommandBus>,
    request: BootRequest,
    body: SetDurableCellApplicationStateRequest,
    start: bool,
) -> Result<BootResponse> {
    let (idempotency_key, request_id) = request_identity(&request)?;
    let (organization_id, project_id, environment_id) = scope(&request)?;
    let application_id = application_id(&request)?;
    let actor_principal_id = actor_principal_id(&request)?;
    let resource_access = resource_access_evaluator(&request.require_auth_principal()?)?;
    let result = if start {
        bus.execute(StartDurableCellApplication {
            organization_id,
            project_id,
            environment_id,
            application_id,
            expected_version: body.expected_version,
            actor_principal_id,
            resource_access,
            idempotency_key,
            request_id,
        })
        .await?
    } else {
        bus.execute(StopDurableCellApplication {
            organization_id,
            project_id,
            environment_id,
            application_id,
            expected_version: body.expected_version,
            actor_principal_id,
            resource_access,
            idempotency_key,
            request_id,
        })
        .await?
    };
    match result {
        Ok(result) => BootResponse::json(&DurableCellApplicationMutationResponse::from(result)),
        Err(error) => application_error_response(error, request_id),
    }
}

fn scope(request: &BootRequest) -> Result<(OrganizationId, ProjectId, EnvironmentId)> {
    Ok((
        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?),
        ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
        EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?),
    ))
}

fn application_id(request: &BootRequest) -> Result<DurableCellApplicationId> {
    Ok(DurableCellApplicationId::from_uuid(
        request.param_as::<Uuid>("application_id")?,
    ))
}

fn revision_id(request: &BootRequest) -> Result<DurableCellApplicationRevisionId> {
    Ok(DurableCellApplicationRevisionId::from_uuid(
        request.param_as::<Uuid>("revision_id")?,
    ))
}

fn list_limit(request: &BootRequest) -> Result<usize> {
    let limit = request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_DURABLE_CELL_APPLICATION_LIST_LIMIT);
    if limit == 0 || limit > MAXIMUM_DURABLE_CELL_APPLICATION_LIST_LIMIT {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAXIMUM_DURABLE_CELL_APPLICATION_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}
