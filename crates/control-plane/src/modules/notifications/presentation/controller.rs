use super::dto::{
    MarkNotificationReadRequest, NotificationMutationResponse, NotificationPageResponse,
    NotificationResponse,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{
    authenticated_actor, resource_access_evaluator, with_deferred_resource_scope,
    DeferredResourceScope, OrganizationTenantGuard,
};
use crate::modules::notifications::{
    GetNotification, ListNotifications, MarkNotificationRead, DEFAULT_NOTIFICATION_LIMIT,
    MAXIMUM_NOTIFICATION_LIMIT,
};
use crate::modules::shared_kernel::domain::{NotificationId, OrganizationId};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, HttpMethod, QueryBus,
    Result, RouteDefinition, AUTH_SCOPES_METADATA,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn notification_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_bus = Arc::clone(&bus);
    let list_route = RouteDefinition::new(
        HttpMethod::Get,
        "/{organization_id}/notifications",
        move |request: BootRequest| {
            let bus = Arc::clone(&list_bus);
            async move {
                let parameters: NotificationParameters = request.query()?;
                if parameters.limit == 0 || parameters.limit > MAXIMUM_NOTIFICATION_LIMIT {
                    return Err(BootError::BadRequest(format!(
                        "notification limit must be between 1 and {MAXIMUM_NOTIFICATION_LIMIT}"
                    )));
                }
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let principal = request.require_auth_principal()?;
                let actor = authenticated_actor(&principal)?;
                let request_id = request_id(&request)?;
                match bus
                    .execute(ListNotifications {
                        organization_id,
                        actor_principal_id: actor.principal_id,
                        resource_access: resource_access_evaluator(&principal)?,
                        unread_only: parameters.unread_only,
                        cursor: parameters.cursor,
                        limit: parameters.limit,
                    })
                    .await?
                {
                    Ok(page) => BootResponse::json(&NotificationPageResponse::from(page)),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?;
    let get_route = RouteDefinition::new(
        HttpMethod::Get,
        "/{organization_id}/notifications/{notification_id}",
        move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let principal = request.require_auth_principal()?;
                let request_id = request_id(&request)?;
                match bus
                    .execute(GetNotification {
                        organization_id,
                        notification_id: NotificationId::from_uuid(
                            request.param_as::<Uuid>("notification_id")?,
                        ),
                        actor_principal_id: authenticated_actor(&principal)?.principal_id,
                        resource_access: resource_access_evaluator(&principal)?,
                    })
                    .await?
                {
                    Ok(notification) => {
                        BootResponse::json(&NotificationResponse::from(notification))
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?;
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .route(with_deferred_resource_scope(
            list_route,
            DeferredResourceScope::Personal,
        )?)?
        .route(with_deferred_resource_scope(
            get_route,
            DeferredResourceScope::Personal,
        )?)
}

pub fn notification_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let route = RouteDefinition::new(
        HttpMethod::Post,
        "/{organization_id}/notifications/{notification_id}/read",
        move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let body: MarkNotificationReadRequest = request.json_with_content_type()?;
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let notification_id =
                    NotificationId::from_uuid(request.param_as::<Uuid>("notification_id")?);
                let principal = request.require_auth_principal()?;
                let request_id = request_id(&request)?;
                match bus
                    .execute(MarkNotificationRead {
                        organization_id,
                        notification_id,
                        expected_version: body.expected_version,
                        actor_principal_id: authenticated_actor(&principal)?.principal_id,
                        resource_access: resource_access_evaluator(&principal)?,
                        idempotency_key: request
                            .header("idempotency-key")
                            .filter(|value| !value.is_empty())
                            .ok_or_else(|| {
                                BootError::BadRequest("idempotency-key header is required".into())
                            })?
                            .to_owned(),
                        request_id,
                    })
                    .await?
                {
                    Ok(result) => BootResponse::json(&NotificationMutationResponse::from(result)),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?;
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(
            AUTH_SCOPES_METADATA,
            vec![ApiTokenScope::NOTIFICATION_WRITE],
        )?
        .route(with_deferred_resource_scope(
            route,
            DeferredResourceScope::Personal,
        )?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NotificationParameters {
    #[serde(default)]
    unread_only: bool,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

const fn default_limit() -> usize {
    DEFAULT_NOTIFICATION_LIMIT
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
