use super::outbound_subscription::outbound_subscription_not_found;
use super::MAXIMUM_NOTIFICATION_LIMIT;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::notifications::domain::{
    IOutboundNotificationRepository, OutboundNotificationSubscription,
    OutboundNotificationSubscriptionCursor, OutboundNotificationSubscriptionPage,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    NotificationSubscriptionId, OrganizationId, PrincipalId,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

const STORAGE_PAGE_SIZE: usize = MAXIMUM_NOTIFICATION_LIMIT;

#[derive(Debug, Clone)]
pub struct GetOutboundNotificationSubscription {
    pub organization_id: OrganizationId,
    pub subscription_id: NotificationSubscriptionId,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetOutboundNotificationSubscription {
    type Output = ApplicationResult<OutboundNotificationSubscription>;
}

pub struct GetOutboundNotificationSubscriptionHandler {
    notifications: Arc<dyn IOutboundNotificationRepository>,
}

impl GetOutboundNotificationSubscriptionHandler {
    pub fn new(notifications: Arc<dyn IOutboundNotificationRepository>) -> Self {
        Self { notifications }
    }
}

impl QueryHandler<GetOutboundNotificationSubscription>
    for GetOutboundNotificationSubscriptionHandler
{
    fn execute(
        &self,
        query: GetOutboundNotificationSubscription,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<OutboundNotificationSubscription>>,
    > {
        let notifications = Arc::clone(&self.notifications);
        Box::pin(async move {
            if query.organization_id.as_uuid().is_nil()
                || query.subscription_id.as_uuid().is_nil()
                || query.actor_principal_id.as_uuid().is_nil()
            {
                return Ok(Err(ApplicationError::Invalid(
                    "outbound notification subscription identity is invalid".into(),
                )));
            }
            let subscription = match notifications
                .find_subscription(
                    query.organization_id,
                    query.actor_principal_id,
                    query.subscription_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None)
                | Err(crate::modules::shared_kernel::domain::RepositoryError::NotFound) => {
                    return Ok(Err(outbound_subscription_not_found()))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if !is_visible(&subscription, &query.resource_access) {
                return Ok(Err(outbound_subscription_not_found()));
            }
            Ok(Ok(subscription))
        })
    }
}

#[derive(Debug, Clone)]
pub struct ListOutboundNotificationSubscriptions {
    pub organization_id: OrganizationId,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub cursor: Option<String>,
    pub limit: usize,
}

impl Query for ListOutboundNotificationSubscriptions {
    type Output = ApplicationResult<OutboundNotificationSubscriptionPage>;
}

pub struct ListOutboundNotificationSubscriptionsHandler {
    notifications: Arc<dyn IOutboundNotificationRepository>,
}

impl ListOutboundNotificationSubscriptionsHandler {
    pub fn new(notifications: Arc<dyn IOutboundNotificationRepository>) -> Self {
        Self { notifications }
    }
}

impl QueryHandler<ListOutboundNotificationSubscriptions>
    for ListOutboundNotificationSubscriptionsHandler
{
    fn execute(
        &self,
        query: ListOutboundNotificationSubscriptions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<OutboundNotificationSubscriptionPage>>,
    > {
        let notifications = Arc::clone(&self.notifications);
        Box::pin(async move {
            if query.organization_id.as_uuid().is_nil()
                || query.actor_principal_id.as_uuid().is_nil()
                || query.limit == 0
                || query.limit > MAXIMUM_NOTIFICATION_LIMIT
            {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "outbound notification subscription identity is invalid or limit is outside 1..={MAXIMUM_NOTIFICATION_LIMIT}"
                ))));
            }
            let mut after = match query
                .cursor
                .as_deref()
                .map(OutboundNotificationSubscriptionCursor::parse)
            {
                Some(Ok(cursor)) => Some(cursor),
                Some(Err(error)) => return Ok(Err(ApplicationError::Invalid(error))),
                None => None,
            };
            let mut visible = Vec::with_capacity(query.limit + 1);
            loop {
                let page = match notifications
                    .list_subscription_page(
                        query.organization_id,
                        query.actor_principal_id,
                        after,
                        STORAGE_PAGE_SIZE,
                    )
                    .await
                {
                    Ok(page) => page,
                    Err(error) => return Ok(Err(error.into())),
                };
                let raw_len = page.len();
                after = page
                    .last()
                    .map(OutboundNotificationSubscriptionCursor::after);
                visible.extend(
                    page.into_iter()
                        .filter(|subscription| is_visible(subscription, &query.resource_access))
                        .take(query.limit + 1 - visible.len()),
                );
                if visible.len() > query.limit || raw_len < STORAGE_PAGE_SIZE {
                    break;
                }
            }
            let next_cursor = (visible.len() > query.limit).then(|| {
                OutboundNotificationSubscriptionCursor::after(&visible[query.limit - 1]).encode()
            });
            visible.truncate(query.limit);
            Ok(Ok(OutboundNotificationSubscriptionPage {
                subscriptions: visible,
                next_cursor,
            }))
        })
    }
}

fn is_visible(
    subscription: &OutboundNotificationSubscription,
    resource_access: &ResourceAccessEvaluator,
) -> bool {
    let target = subscription.definition.spec().target;
    resource_access.allows(ResourceGrantScope::Environment {
        project_id: target.project_id,
        environment_id: target.environment_id,
    })
}
