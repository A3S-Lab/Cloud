use super::get_notification::not_found;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::notifications::domain::{
    INotificationRepository, MarkNotificationReadWrite, Notification,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, NotificationId, OrganizationId, PrincipalId,
};
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct MarkNotificationRead {
    pub organization_id: OrganizationId,
    pub notification_id: NotificationId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for MarkNotificationRead {
    type Output = ApplicationResult<MarkNotificationReadResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkNotificationReadResult {
    pub notification: Notification,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NotificationReadEvent {
    notification_id: NotificationId,
    source_event_id: Uuid,
}

pub struct MarkNotificationReadHandler {
    notifications: Arc<dyn INotificationRepository>,
}

impl MarkNotificationReadHandler {
    pub fn new(notifications: Arc<dyn INotificationRepository>) -> Self {
        Self { notifications }
    }
}

impl CommandHandler<MarkNotificationRead> for MarkNotificationReadHandler {
    fn execute(
        &self,
        command: MarkNotificationRead,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<MarkNotificationReadResult>>>
    {
        let notifications = Arc::clone(&self.notifications);
        Box::pin(async move {
            if command.actor_principal_id.as_uuid().is_nil()
                || command.request_id.is_nil()
                || command.expected_version == 0
            {
                return Ok(Err(ApplicationError::Invalid(
                    "notification actor, request, and expected version are invalid".into(),
                )));
            }
            let existing = match notifications
                .find(
                    command.organization_id,
                    command.actor_principal_id,
                    command.notification_id,
                )
                .await
            {
                Ok(Some(notification))
                    if notification.scope.is_visible_to(&command.resource_access) =>
                {
                    notification
                }
                Ok(Some(_)) | Ok(None) => return Ok(Err(not_found())),
                Err(error) => return Ok(Err(error.into())),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "notificationId": command.notification_id,
                "expectedVersion": command.expected_version,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/notifications/{}/read",
                    command.organization_id, command.notification_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match notifications.replay_mark_read(&idempotency).await {
                Ok(Some(replayed))
                    if replayed.value.organization_id == command.organization_id
                        && replayed.value.id == command.notification_id
                        && replayed.value.recipient_principal_id == command.actor_principal_id =>
                {
                    return Ok(Ok(MarkNotificationReadResult {
                        notification: replayed.value,
                        replayed: true,
                    }));
                }
                Ok(Some(_)) => {
                    return Err(BootError::Internal(
                        "notification read replay changed its immutable identity".into(),
                    ));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let notification = match existing.mark_read(command.expected_version, Utc::now()) {
                Ok(notification) => notification,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let read_at = notification.read_at.expect("read transition time");
            let event = DomainEventEnvelope {
                event_id: Uuid::now_v7(),
                event_key: "notification.inbox.read".into(),
                schema_version: 1,
                organization_id: notification.organization_id.as_uuid(),
                aggregate_id: notification.id.as_uuid(),
                aggregate_version: notification.aggregate_version,
                occurred_at: read_at,
                correlation_id: command.request_id,
                causation_id: Some(notification.source_event_id),
                payload: serde_json::to_value(NotificationReadEvent {
                    notification_id: notification.id,
                    source_event_id: notification.source_event_id,
                })
                .map_err(|error| BootError::Internal(error.to_string()))?,
            };
            let result = match notifications
                .mark_read(MarkNotificationReadWrite {
                    notification,
                    expected_version: command.expected_version,
                    actor_principal_id: command.actor_principal_id,
                    event,
                    idempotency,
                    request_id: command.request_id,
                })
                .await
            {
                Ok(result) => result,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(MarkNotificationReadResult {
                notification: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
