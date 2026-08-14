use super::tool_result;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::notifications::presentation::{
    NotificationMutationResponse, NotificationPageResponse, NotificationResponse,
};
use crate::modules::notifications::{GetNotification, ListNotifications, MarkNotificationRead};
use crate::modules::shared_kernel::domain::{NotificationId, OrganizationId, PrincipalId};
use a3s_boot::{CommandBus, QueryBus, Result};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationListArguments {
    #[serde(default)]
    unread_only: bool,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(
        default = "super::arguments::default_list_limit",
        deserialize_with = "super::arguments::deserialize_list_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationArguments {
    notification_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkNotificationReadArguments {
    notification_id: Uuid,
    #[serde(deserialize_with = "super::arguments::deserialize_expected_version")]
    expected_version: u64,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

pub async fn list(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: NotificationListArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListNotifications {
            organization_id,
            actor_principal_id,
            resource_access,
            unread_only: arguments.unread_only,
            cursor: arguments.cursor,
            limit: arguments.limit,
        })
        .await?
    {
        Ok(page) => tool_result::success(200, NotificationPageResponse::from(page), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: NotificationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetNotification {
            organization_id,
            notification_id: NotificationId::from_uuid(arguments.notification_id),
            actor_principal_id,
            resource_access,
        })
        .await?
    {
        Ok(notification) => {
            tool_result::success(200, NotificationResponse::from(notification), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn mark_read(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: MarkNotificationReadArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(MarkNotificationRead {
            organization_id,
            notification_id: NotificationId::from_uuid(arguments.notification_id),
            expected_version: arguments.expected_version,
            actor_principal_id,
            resource_access,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            tool_result::success(200, NotificationMutationResponse::from(result), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}
