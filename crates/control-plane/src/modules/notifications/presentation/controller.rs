use super::dto::{
    MarkNotificationReadRequest, NotificationAlertPolicyMutationResponse,
    NotificationAlertPolicyPageResponse, NotificationAlertPolicyResponse,
    NotificationMutationResponse, NotificationPageResponse, NotificationResponse,
    OutboundNotificationSubscriptionMutationResponse, OutboundNotificationSubscriptionPageResponse,
    OutboundNotificationSubscriptionResponse, RevokeNotificationAlertPolicyRequest,
    RevokeOutboundNotificationSubscriptionRequest,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{
    authenticated_actor, resource_access_evaluator, with_deferred_resource_scope,
    DeferredResourceScope, OrganizationTenantGuard,
};
use crate::modules::notifications::{
    CreateNotificationAlertPolicy, CreateOutboundNotificationSubscription, GetNotification,
    GetNotificationAlertPolicy, GetOutboundNotificationSubscription, ListNotificationAlertPolicies,
    ListNotifications, ListOutboundNotificationSubscriptions, MarkNotificationRead,
    RevokeNotificationAlertPolicy, RevokeOutboundNotificationSubscription,
    DEFAULT_NOTIFICATION_LIMIT, MAXIMUM_NOTIFICATION_LIMIT,
    NOTIFICATION_ALERT_POLICY_MAX_ACL_BYTES, OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES,
};
use crate::modules::shared_kernel::domain::{
    NotificationAlertPolicyId, NotificationId, NotificationSubscriptionId, OrganizationId,
};
use crate::presentation::{application_error_response, bounded_acl_document};
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, HttpMethod, QueryBus,
    Result, RouteDefinition, AUTH_SCOPES_METADATA,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn notification_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_bus = Arc::clone(&bus);
    let get_notification_bus = Arc::clone(&bus);
    let list_alert_policies_bus = Arc::clone(&bus);
    let get_alert_policy_bus = Arc::clone(&bus);
    let list_subscriptions_bus = Arc::clone(&bus);
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
            let bus = Arc::clone(&get_notification_bus);
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
    let list_alert_policies_route = RouteDefinition::new(
        HttpMethod::Get,
        "/{organization_id}/notification-alert-policies",
        move |request: BootRequest| {
            let bus = Arc::clone(&list_alert_policies_bus);
            async move {
                let parameters: AlertPolicyParameters = request.query()?;
                if parameters.limit == 0 || parameters.limit > MAXIMUM_NOTIFICATION_LIMIT {
                    return Err(BootError::BadRequest(format!(
                        "notification alert policy limit must be between 1 and {MAXIMUM_NOTIFICATION_LIMIT}"
                    )));
                }
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let principal = request.require_auth_principal()?;
                let request_id = request_id(&request)?;
                match bus
                    .execute(ListNotificationAlertPolicies {
                        organization_id,
                        actor_principal_id: authenticated_actor(&principal)?.principal_id,
                        resource_access: resource_access_evaluator(&principal)?,
                        cursor: parameters.cursor,
                        limit: parameters.limit,
                    })
                    .await?
                {
                    Ok(page) => {
                        BootResponse::json(&NotificationAlertPolicyPageResponse::from(page))
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?;
    let get_alert_policy_route = RouteDefinition::new(
        HttpMethod::Get,
        "/{organization_id}/notification-alert-policies/{policy_id}",
        move |request: BootRequest| {
            let bus = Arc::clone(&get_alert_policy_bus);
            async move {
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let principal = request.require_auth_principal()?;
                let request_id = request_id(&request)?;
                match bus
                    .execute(GetNotificationAlertPolicy {
                        organization_id,
                        policy_id: NotificationAlertPolicyId::from_uuid(
                            request.param_as::<Uuid>("policy_id")?,
                        ),
                        actor_principal_id: authenticated_actor(&principal)?.principal_id,
                        resource_access: resource_access_evaluator(&principal)?,
                    })
                    .await?
                {
                    Ok(policy) => {
                        BootResponse::json(&NotificationAlertPolicyResponse::from(policy))
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?;
    let list_subscriptions_route = RouteDefinition::new(
        HttpMethod::Get,
        "/{organization_id}/notification-outbound-subscriptions",
        move |request: BootRequest| {
            let bus = Arc::clone(&list_subscriptions_bus);
            async move {
                let parameters: OutboundSubscriptionParameters = request.query()?;
                if parameters.limit == 0 || parameters.limit > MAXIMUM_NOTIFICATION_LIMIT {
                    return Err(BootError::BadRequest(format!(
                        "outbound notification subscription limit must be between 1 and {MAXIMUM_NOTIFICATION_LIMIT}"
                    )));
                }
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let principal = request.require_auth_principal()?;
                let request_id = request_id(&request)?;
                match bus
                    .execute(ListOutboundNotificationSubscriptions {
                        organization_id,
                        actor_principal_id: authenticated_actor(&principal)?.principal_id,
                        resource_access: resource_access_evaluator(&principal)?,
                        cursor: parameters.cursor,
                        limit: parameters.limit,
                    })
                    .await?
                {
                    Ok(page) => BootResponse::json(
                        &OutboundNotificationSubscriptionPageResponse::from(page),
                    ),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?;
    let get_subscription_route = RouteDefinition::new(
        HttpMethod::Get,
        "/{organization_id}/notification-outbound-subscriptions/{subscription_id}",
        move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let principal = request.require_auth_principal()?;
                let request_id = request_id(&request)?;
                match bus
                    .execute(GetOutboundNotificationSubscription {
                        organization_id,
                        subscription_id: NotificationSubscriptionId::from_uuid(
                            request.param_as::<Uuid>("subscription_id")?,
                        ),
                        actor_principal_id: authenticated_actor(&principal)?.principal_id,
                        resource_access: resource_access_evaluator(&principal)?,
                    })
                    .await?
                {
                    Ok(subscription) => BootResponse::json(
                        &OutboundNotificationSubscriptionResponse::from(subscription),
                    ),
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
        )?)?
        .route(with_deferred_resource_scope(
            list_alert_policies_route,
            DeferredResourceScope::Personal,
        )?)?
        .route(with_deferred_resource_scope(
            get_alert_policy_route,
            DeferredResourceScope::Personal,
        )?)?
        .route(with_deferred_resource_scope(
            list_subscriptions_route,
            DeferredResourceScope::Personal,
        )?)?
        .route(with_deferred_resource_scope(
            get_subscription_route,
            DeferredResourceScope::Personal,
        )?)
}

pub fn notification_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let mark_read_bus = Arc::clone(&bus);
    let create_alert_policy_bus = Arc::clone(&bus);
    let revoke_alert_policy_bus = Arc::clone(&bus);
    let create_subscription_bus = Arc::clone(&bus);
    let route = RouteDefinition::new(
        HttpMethod::Post,
        "/{organization_id}/notifications/{notification_id}/read",
        move |request: BootRequest| {
            let bus = Arc::clone(&mark_read_bus);
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
                        idempotency_key: idempotency_key(&request)?,
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
    let create_alert_policy_route = RouteDefinition::new(
        HttpMethod::Post,
        "/{organization_id}/notification-alert-policies",
        move |request: BootRequest| {
            let bus = Arc::clone(&create_alert_policy_bus);
            async move {
                let definition_acl = bounded_acl_document(
                    &request,
                    NOTIFICATION_ALERT_POLICY_MAX_ACL_BYTES,
                    "Notification alert policy",
                )?;
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let principal = request.require_auth_principal()?;
                let request_id = request_id(&request)?;
                match bus
                    .execute(CreateNotificationAlertPolicy {
                        organization_id,
                        definition_acl,
                        actor_principal_id: authenticated_actor(&principal)?.principal_id,
                        resource_access: resource_access_evaluator(&principal)?,
                        idempotency_key: idempotency_key(&request)?,
                        request_id,
                    })
                    .await?
                {
                    Ok(result) => BootResponse::json_with_status(
                        if result.replayed { 200 } else { 201 },
                        &NotificationAlertPolicyMutationResponse::from(result),
                    ),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?;
    let revoke_alert_policy_route = RouteDefinition::new(
        HttpMethod::Post,
        "/{organization_id}/notification-alert-policies/{policy_id}/revoke",
        move |request: BootRequest| {
            let bus = Arc::clone(&revoke_alert_policy_bus);
            async move {
                let body: RevokeNotificationAlertPolicyRequest =
                    request.json_with_content_type()?;
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let principal = request.require_auth_principal()?;
                let request_id = request_id(&request)?;
                match bus
                    .execute(RevokeNotificationAlertPolicy {
                        organization_id,
                        policy_id: NotificationAlertPolicyId::from_uuid(
                            request.param_as::<Uuid>("policy_id")?,
                        ),
                        expected_version: body.expected_version,
                        actor_principal_id: authenticated_actor(&principal)?.principal_id,
                        resource_access: resource_access_evaluator(&principal)?,
                        idempotency_key: idempotency_key(&request)?,
                        request_id,
                    })
                    .await?
                {
                    Ok(result) => {
                        BootResponse::json(&NotificationAlertPolicyMutationResponse::from(result))
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?;
    let create_subscription_route = RouteDefinition::new(
        HttpMethod::Post,
        "/{organization_id}/notification-outbound-subscriptions",
        move |request: BootRequest| {
            let bus = Arc::clone(&create_subscription_bus);
            async move {
                let definition_acl = bounded_acl_document(
                    &request,
                    OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES,
                    "Outbound notification subscription",
                )?;
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let principal = request.require_auth_principal()?;
                let request_id = request_id(&request)?;
                match bus
                    .execute(CreateOutboundNotificationSubscription {
                        organization_id,
                        definition_acl,
                        actor_principal_id: authenticated_actor(&principal)?.principal_id,
                        resource_access: resource_access_evaluator(&principal)?,
                        idempotency_key: idempotency_key(&request)?,
                        request_id,
                    })
                    .await?
                {
                    Ok(result) => BootResponse::json_with_status(
                        if result.replayed { 200 } else { 201 },
                        &OutboundNotificationSubscriptionMutationResponse::from(result),
                    ),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?;
    let revoke_subscription_route = RouteDefinition::new(
        HttpMethod::Post,
        "/{organization_id}/notification-outbound-subscriptions/{subscription_id}/revoke",
        move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let body: RevokeOutboundNotificationSubscriptionRequest =
                    request.json_with_content_type()?;
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let principal = request.require_auth_principal()?;
                let request_id = request_id(&request)?;
                match bus
                    .execute(RevokeOutboundNotificationSubscription {
                        organization_id,
                        subscription_id: NotificationSubscriptionId::from_uuid(
                            request.param_as::<Uuid>("subscription_id")?,
                        ),
                        expected_version: body.expected_version,
                        actor_principal_id: authenticated_actor(&principal)?.principal_id,
                        resource_access: resource_access_evaluator(&principal)?,
                        idempotency_key: idempotency_key(&request)?,
                        request_id,
                    })
                    .await?
                {
                    Ok(result) => BootResponse::json(
                        &OutboundNotificationSubscriptionMutationResponse::from(result),
                    ),
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
        )?)?
        .route(with_deferred_resource_scope(
            create_alert_policy_route,
            DeferredResourceScope::Personal,
        )?)?
        .route(with_deferred_resource_scope(
            revoke_alert_policy_route,
            DeferredResourceScope::Personal,
        )?)?
        .route(with_deferred_resource_scope(
            create_subscription_route,
            DeferredResourceScope::Personal,
        )?)?
        .route(with_deferred_resource_scope(
            revoke_subscription_route,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OutboundSubscriptionParameters {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AlertPolicyParameters {
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

fn idempotency_key(request: &BootRequest) -> Result<String> {
    request
        .header("idempotency-key")
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| BootError::BadRequest("idempotency-key header is required".into()))
}
