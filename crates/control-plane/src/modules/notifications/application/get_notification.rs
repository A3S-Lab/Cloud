use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::notifications::domain::{INotificationRepository, Notification};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{NotificationId, OrganizationId, PrincipalId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetNotification {
    pub organization_id: OrganizationId,
    pub notification_id: NotificationId,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetNotification {
    type Output = ApplicationResult<Notification>;
}

pub struct GetNotificationHandler {
    notifications: Arc<dyn INotificationRepository>,
}

impl GetNotificationHandler {
    pub fn new(notifications: Arc<dyn INotificationRepository>) -> Self {
        Self { notifications }
    }
}

impl QueryHandler<GetNotification> for GetNotificationHandler {
    fn execute(
        &self,
        query: GetNotification,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Notification>>> {
        let notifications = Arc::clone(&self.notifications);
        Box::pin(async move {
            let notification = match notifications
                .find(
                    query.organization_id,
                    query.actor_principal_id,
                    query.notification_id,
                )
                .await
            {
                Ok(Some(notification)) => notification,
                Ok(None) => return Ok(Err(not_found())),
                Err(error) => return Ok(Err(error.into())),
            };
            if !notification.scope.is_visible_to(&query.resource_access) {
                return Ok(Err(not_found()));
            }
            Ok(Ok(notification))
        })
    }
}

pub(super) fn not_found() -> ApplicationError {
    ApplicationError::NotFound("notification not found".into())
}
