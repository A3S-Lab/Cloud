use crate::modules::identity::domain::repositories::{
    IMembershipRepository, IResourceGrantRepository,
};
use crate::modules::identity::domain::value_objects::{MembershipRole, ResourceGrantScope};
use crate::modules::notifications::application::INotificationOutboxIdentityAccess;
use crate::modules::notifications::{NotificationAccess, NotificationAccessScope};
use crate::modules::shared_kernel::domain::{
    MembershipId, OrganizationId, PrincipalId, RepositoryError,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Notifications' sole Identity membership/grant adapter for outbox projection.
pub struct IdentityNotificationOutboxIdentityAccessAdapter {
    memberships: Arc<dyn IMembershipRepository>,
    resource_grants: Arc<dyn IResourceGrantRepository>,
}

impl IdentityNotificationOutboxIdentityAccessAdapter {
    pub fn new(
        memberships: Arc<dyn IMembershipRepository>,
        resource_grants: Arc<dyn IResourceGrantRepository>,
    ) -> Self {
        Self {
            memberships,
            resource_grants,
        }
    }
}

#[async_trait]
impl INotificationOutboxIdentityAccess for IdentityNotificationOutboxIdentityAccessAdapter {
    async fn membership_inbox_is_projectable(
        &self,
        organization_id: OrganizationId,
        membership_id: MembershipId,
        expected_principal_id: PrincipalId,
    ) -> Result<bool, RepositoryError> {
        let membership = self
            .memberships
            .find_membership(organization_id, membership_id)
            .await?
            .ok_or_else(|| {
                RepositoryError::Storage("notification source membership no longer exists".into())
            })?;
        if membership.membership.principal_id != expected_principal_id {
            return Err(RepositoryError::Storage(
                "notification source membership principal is inconsistent".into(),
            ));
        }
        Ok(membership.membership.is_active())
    }

    async fn alert_access_for_principal(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Option<NotificationAccess>, RepositoryError> {
        let Some(membership) = self
            .memberships
            .find_active_membership_by_principal(organization_id, principal_id)
            .await?
        else {
            return Ok(None);
        };
        let grants = self
            .resource_grants
            .list_active_resource_grants_for_membership(organization_id, membership.id)
            .await?;
        Ok(Some(notification_access_for_membership(
            membership.role,
            grants.into_iter().map(|grant| grant.scope),
        )))
    }
}

fn notification_access_for_membership(
    role: MembershipRole,
    grants: impl IntoIterator<Item = ResourceGrantScope>,
) -> NotificationAccess {
    if role == MembershipRole::Restricted {
        NotificationAccess::restricted(grants.into_iter().map(|scope| match scope {
            ResourceGrantScope::Project { project_id } => {
                NotificationAccessScope::Project { project_id }
            }
            ResourceGrantScope::Environment {
                project_id,
                environment_id,
            } => NotificationAccessScope::Environment {
                project_id,
                environment_id,
            },
            ResourceGrantScope::Node { node_id } => NotificationAccessScope::Node { node_id },
        }))
    } else {
        NotificationAccess::organization_wide()
    }
}
