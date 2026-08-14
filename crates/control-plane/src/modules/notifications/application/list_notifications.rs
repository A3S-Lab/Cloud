use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::notifications::domain::{
    INotificationRepository, NotificationCursor, NotificationPage,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

pub const DEFAULT_NOTIFICATION_LIMIT: usize = 50;
pub const MAXIMUM_NOTIFICATION_LIMIT: usize = 200;
const STORAGE_PAGE_SIZE: usize = 200;

#[derive(Debug, Clone)]
pub struct ListNotifications {
    pub organization_id: OrganizationId,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub unread_only: bool,
    pub cursor: Option<String>,
    pub limit: usize,
}

impl Query for ListNotifications {
    type Output = ApplicationResult<NotificationPage>;
}

pub struct ListNotificationsHandler {
    notifications: Arc<dyn INotificationRepository>,
}

impl ListNotificationsHandler {
    pub fn new(notifications: Arc<dyn INotificationRepository>) -> Self {
        Self { notifications }
    }
}

impl QueryHandler<ListNotifications> for ListNotificationsHandler {
    fn execute(
        &self,
        query: ListNotifications,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<NotificationPage>>> {
        let notifications = Arc::clone(&self.notifications);
        Box::pin(async move {
            if query.actor_principal_id.as_uuid().is_nil()
                || query.limit == 0
                || query.limit > MAXIMUM_NOTIFICATION_LIMIT
            {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "notification actor must be non-nil and limit must be between 1 and {MAXIMUM_NOTIFICATION_LIMIT}"
                ))));
            }
            let mut after = match query.cursor.as_deref().map(NotificationCursor::parse) {
                Some(Ok(cursor)) => Some(cursor),
                Some(Err(error)) => return Ok(Err(ApplicationError::Invalid(error))),
                None => None,
            };
            let mut visible = Vec::with_capacity(query.limit + 1);
            loop {
                let page = match notifications
                    .list_page(
                        query.organization_id,
                        query.actor_principal_id,
                        query.unread_only,
                        after,
                        STORAGE_PAGE_SIZE,
                    )
                    .await
                {
                    Ok(page) => page,
                    Err(error) => return Ok(Err(error.into())),
                };
                let raw_len = page.len();
                after = page.last().map(NotificationCursor::after);
                visible.extend(
                    page.into_iter()
                        .filter(|notification| {
                            notification.scope.is_visible_to(&query.resource_access)
                        })
                        .take(query.limit + 1 - visible.len()),
                );
                if visible.len() > query.limit || raw_len < STORAGE_PAGE_SIZE {
                    break;
                }
            }
            let next_cursor = (visible.len() > query.limit)
                .then(|| NotificationCursor::after(&visible[query.limit - 1]).encode());
            visible.truncate(query.limit);
            Ok(Ok(NotificationPage {
                notifications: visible,
                next_cursor,
            }))
        })
    }
}
