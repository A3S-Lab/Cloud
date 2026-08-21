use super::alert_policy::alert_policy_not_found;
use super::MAXIMUM_NOTIFICATION_LIMIT;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::notifications::domain::{
    INotificationAlertPolicyRepository, NotificationAlertPolicy, NotificationAlertPolicyCursor,
    NotificationAlertPolicyPage,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    NotificationAlertPolicyId, OrganizationId, PrincipalId,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

const STORAGE_PAGE_SIZE: usize = MAXIMUM_NOTIFICATION_LIMIT;

#[derive(Debug, Clone)]
pub struct GetNotificationAlertPolicy {
    pub organization_id: OrganizationId,
    pub policy_id: NotificationAlertPolicyId,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetNotificationAlertPolicy {
    type Output = ApplicationResult<NotificationAlertPolicy>;
}

pub struct GetNotificationAlertPolicyHandler {
    notifications: Arc<dyn INotificationAlertPolicyRepository>,
}

impl GetNotificationAlertPolicyHandler {
    pub fn new(notifications: Arc<dyn INotificationAlertPolicyRepository>) -> Self {
        Self { notifications }
    }
}

impl QueryHandler<GetNotificationAlertPolicy> for GetNotificationAlertPolicyHandler {
    fn execute(
        &self,
        query: GetNotificationAlertPolicy,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<NotificationAlertPolicy>>>
    {
        let notifications = Arc::clone(&self.notifications);
        Box::pin(async move {
            if query.organization_id.as_uuid().is_nil()
                || query.policy_id.as_uuid().is_nil()
                || query.actor_principal_id.as_uuid().is_nil()
            {
                return Ok(Err(ApplicationError::Invalid(
                    "notification alert policy identity is invalid".into(),
                )));
            }
            let policy = match notifications
                .find_alert_policy(
                    query.organization_id,
                    query.actor_principal_id,
                    query.policy_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None)
                | Err(crate::modules::shared_kernel::domain::RepositoryError::NotFound) => {
                    return Ok(Err(alert_policy_not_found()))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if !is_visible(&policy, &query.resource_access) {
                return Ok(Err(alert_policy_not_found()));
            }
            Ok(Ok(policy))
        })
    }
}

#[derive(Debug, Clone)]
pub struct ListNotificationAlertPolicies {
    pub organization_id: OrganizationId,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub cursor: Option<String>,
    pub limit: usize,
}

impl Query for ListNotificationAlertPolicies {
    type Output = ApplicationResult<NotificationAlertPolicyPage>;
}

pub struct ListNotificationAlertPoliciesHandler {
    notifications: Arc<dyn INotificationAlertPolicyRepository>,
}

impl ListNotificationAlertPoliciesHandler {
    pub fn new(notifications: Arc<dyn INotificationAlertPolicyRepository>) -> Self {
        Self { notifications }
    }
}

impl QueryHandler<ListNotificationAlertPolicies> for ListNotificationAlertPoliciesHandler {
    fn execute(
        &self,
        query: ListNotificationAlertPolicies,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<NotificationAlertPolicyPage>>,
    > {
        let notifications = Arc::clone(&self.notifications);
        Box::pin(async move {
            if query.organization_id.as_uuid().is_nil()
                || query.actor_principal_id.as_uuid().is_nil()
                || query.limit == 0
                || query.limit > MAXIMUM_NOTIFICATION_LIMIT
            {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "notification alert policy identity is invalid or limit is outside 1..={MAXIMUM_NOTIFICATION_LIMIT}"
                ))));
            }
            let mut after = match query
                .cursor
                .as_deref()
                .map(NotificationAlertPolicyCursor::parse)
            {
                Some(Ok(cursor)) => Some(cursor),
                Some(Err(error)) => return Ok(Err(ApplicationError::Invalid(error))),
                None => None,
            };
            let mut visible = Vec::with_capacity(query.limit + 1);
            loop {
                let page = match notifications
                    .list_alert_policy_page(
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
                after = page.last().map(NotificationAlertPolicyCursor::after);
                visible.extend(
                    page.into_iter()
                        .filter(|policy| is_visible(policy, &query.resource_access))
                        .take(query.limit + 1 - visible.len()),
                );
                if visible.len() > query.limit || raw_len < STORAGE_PAGE_SIZE {
                    break;
                }
            }
            let next_cursor = (visible.len() > query.limit)
                .then(|| NotificationAlertPolicyCursor::after(&visible[query.limit - 1]).encode());
            visible.truncate(query.limit);
            Ok(Ok(NotificationAlertPolicyPage {
                policies: visible,
                next_cursor,
            }))
        })
    }
}

fn is_visible(policy: &NotificationAlertPolicy, resource_access: &ResourceAccessEvaluator) -> bool {
    let spec = policy.definition.spec();
    resource_access.allows(ResourceGrantScope::Environment {
        project_id: spec.project_id,
        environment_id: spec.environment_id,
    })
}
